use std::{
	collections::{BTreeMap, HashMap},
	iter::repeat_n,
	ops::Bound,
};

use crate::{
	error,
	interupt::{Interrupt, is_cannonical},
};

pub trait Memory {
	/// The adrees is in [0, size), where size is the size that this memory module was created
	/// with.
	fn read_u8(&mut self, address: u64) -> u8;

	/// The adrees is in [0, size), where size is the size that this memory module was created
	/// with.
	fn write_u8(&mut self, address: u64, value: u8);
}

pub struct ConventionalMemory {
	pages: HashMap<u64, [u8; 1 << 12]>,
}

impl ConventionalMemory {
	pub fn create(_size: u64) -> Self {
		ConventionalMemory {
			pages: HashMap::new(),
		}
	}

	fn get_page(&mut self, address: u64) -> &mut [u8; 1 << 12] {
		let page = address & 0xFFFF_FFFF_FFFF_F000;
		self.pages.entry(page).or_insert([0; 1 << 12])
	}
}

impl Memory for ConventionalMemory {
	fn read_u8(&mut self, address: u64) -> u8 {
		self.get_page(address)[(address & 0xFFF) as usize]
	}

	fn write_u8(&mut self, address: u64, value: u8) {
		self.get_page(address)[(address & 0xFFF) as usize] = value;
	}
}

pub struct ReadOnlyMemory {
	data: Box<[u8]>,
}

impl ReadOnlyMemory {
	pub fn create(prefix: &[u8], size: u64) -> Self {
		match isize::try_from(size) {
			Ok(size) => {
				if prefix.len() > size as usize {
					error::fatal("ROM chip is larger than alloted size");
				}
				// Cast is safe as it is guaranteed to be positive.
				let mut data: Box<[u8]> = repeat_n(0, size as usize).collect();
				data[..prefix.len()].copy_from_slice(prefix);
				Self { data }
			}
			Err(_) => error::fatal(&format!(
				"Conventional Memory size of {size} bytes is greater than the host machine is able to simulate"
			)),
		}
	}
}

impl Memory for ReadOnlyMemory {
	fn read_u8(&mut self, address: u64) -> u8 {
		self.data[address as usize] // In bounds due to MMu check.
	}

	fn write_u8(&mut self, _address: u64, _value: u8) {}
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Range {
	begin: u64,
	end: u64,
}

impl Range {
	fn new(begin: u64, end: u64) -> Range {
		Range { begin, end }
	}
}

pub struct PhysicalMemoryManagementUnit {
	ranges: BTreeMap<Range, Box<dyn Memory>>,
}

impl PhysicalMemoryManagementUnit {
	pub fn new() -> PhysicalMemoryManagementUnit {
		PhysicalMemoryManagementUnit {
			ranges: BTreeMap::new(),
		}
	}

	pub fn add<T>(&mut self, base: u64, size: u64, init: impl FnOnce() -> T)
	where
		T: Memory + 'static,
	{
		let Some(end) = base.checked_add(size) else {
			error::fatal(&format!(
				"Memory base: {base} and memory size: {size} overflows 64-bit address space"
			));
		};
		let range = Range::new(base, end);
		self.ranges.insert(range, Box::new(init()));
	}

	fn read_u8(&mut self, address: u64) -> u8 {
		let mut cursor = self
			.ranges
			.lower_bound_mut(Bound::Excluded(&Range::new(address, u64::MAX)));
		match cursor.prev() {
			Some((range, memory)) if range.end > address => memory.read_u8(address - range.begin),
			_ => 0xFF,
		}
	}

	pub fn read_u64(&mut self, address: u64) -> u64 {
		u64::from_le_bytes(std::array::from_fn(|i| self.read_u8(address + i as u64)))
	}

	pub fn write_u8(&mut self, address: u64, value: u8) {
		let mut cursor = self
			.ranges
			.lower_bound_mut(Bound::Excluded(&Range::new(address, u64::MAX)));
		match cursor.prev() {
			Some((range, memory)) if range.end > address => {
				memory.write_u8(address - range.begin, value)
			}
			_ => (),
		}
	}

	pub fn write_u64(&mut self, address: u64, value: u64) {
		value
			.to_le_bytes()
			.into_iter()
			.enumerate()
			.for_each(|(i, value)| self.write_u8(address + i as u64, value));
	}
}

pub struct MemoryManagementUnit {
	memory_management_unit: PhysicalMemoryManagementUnit,
	paging_table_address: u64,
}

impl MemoryManagementUnit {
	pub fn new(memory_management_unit: PhysicalMemoryManagementUnit) -> MemoryManagementUnit {
		MemoryManagementUnit {
			memory_management_unit,
			paging_table_address: 0,
		}
	}

	fn extract_address(
		&mut self,
		base: u64,
		index: u64,
		virtual_address: u64,
	) -> Result<u64, Interrupt> {
		let entry = self.memory_management_unit.read_u64(base + 8 * index);

		if entry & 1 == 0 {
			return Err(Interrupt::PageFault {
				error_code: 0,
				cr2: virtual_address,
			});
		}

		Ok(entry & 0x7FFF_FFFF_FFFF_F000)
	}

	fn translate(&mut self, virtual_address: u64) -> Result<u64, Interrupt> {
		is_cannonical(virtual_address)?;
		let level_1 = (virtual_address >> 39) & 0x1FF;
		let level_2 = (virtual_address >> 30) & 0x1FF;
		let level_3 = (virtual_address >> 21) & 0x1FF;
		let level_4 = (virtual_address >> 12) & 0x1FF;
		let offset = virtual_address & 0xFFF;
		let level_2_ptr =
			self.extract_address(self.paging_table_address, level_1, virtual_address)?;
		let level_3_ptr = self.extract_address(level_2_ptr, level_2, virtual_address)?;
		let level_4_ptr = self.extract_address(level_3_ptr, level_3, virtual_address)?;
		self.extract_address(level_4_ptr, level_4, virtual_address)
			.map(|page| page + offset)
	}

	pub fn read_u8(&mut self, virtual_address: u64) -> Result<u8, Interrupt> {
		self.translate(virtual_address)
			.map(|address| self.memory_management_unit.read_u8(address))
	}
	
	pub fn read_u16(&mut self, virtual_address: u64) -> Result<u16, Interrupt> {
		std::array::try_from_fn(|i| self.read_u8(virtual_address + i as u64))
			.map(u16::from_le_bytes)
	}
	
	pub fn read_u32(&mut self, virtual_address: u64) -> Result<u32, Interrupt> {
		std::array::try_from_fn(|i| self.read_u8(virtual_address + i as u64))
			.map(u32::from_le_bytes)
	}

	pub fn read_u64(&mut self, virtual_address: u64) -> Result<u64, Interrupt> {
		std::array::try_from_fn(|i| self.read_u8(virtual_address + i as u64))
			.map(u64::from_le_bytes)
	}

	pub fn write_u8(&mut self, virtual_address: u64, value: u8) -> Result<(), Interrupt> {
		self.translate(virtual_address)
			.map(|address| self.memory_management_unit.write_u8(address, value))
	}

	pub fn write_u16(&mut self, virtual_address: u64, value: u16) -> Result<(), Interrupt> {
		value
			.to_le_bytes()
			.into_iter()
			.enumerate()
			.try_for_each(|(i, value)| self.write_u8(virtual_address + i as u64, value))
	}
	
	pub fn write_u32(&mut self, virtual_address: u64, value: u32) -> Result<(), Interrupt> {
		value
			.to_le_bytes()
			.into_iter()
			.enumerate()
			.try_for_each(|(i, value)| self.write_u8(virtual_address + i as u64, value))
	}

	pub fn write_u64(&mut self, virtual_address: u64, value: u64) -> Result<(), Interrupt> {
		value
			.to_le_bytes()
			.into_iter()
			.enumerate()
			.try_for_each(|(i, value)| self.write_u8(virtual_address + i as u64, value))
	}

	pub fn swi4(&mut self, address: u64) {
		self.paging_table_address = address;
	}
}
