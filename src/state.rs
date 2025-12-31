use crate::{
	decode::decode,
	device::PortDevices,
	error::{fatal, info},
	instruction::{Instruction, RegisterSize, Rm},
	interupt::{Interrupt, InteruptDescriptorEntry},
	memory::MemoryManagementUnit,
};

pub struct Registers {
	/// The primary register file which is always available.
	pub primary_registers: [u64; 16],

	/// Used when handling a page fault interrupt. Requires CPL <= 0.
	cr2: u64,
}

impl Registers {
	fn new() -> Registers {
		Registers {
			primary_registers: [0; 16],
			cr2: 0,
		}
	}
}

pub struct ProcessorState {
	/// The register file. Note c3 is not a register but a field in the memory management unit.
	pub registers: Registers,

	/// The memory management unit. This units handles paging translation, so it should just be
	/// used directly with virtual addresses. Holds cr3.
	memory: MemoryManagementUnit,

	/// Simulates the ports of the CPU.
	devices: PortDevices,

	/// Current privilege level:
	cpl: i8,

	/// Where to place the stack for interrupts.
	pub interupt_stack_pointer: u64,

	/// Location of idt:
	pub idt: u64,

	/// The current instruction pointer (virtual address).
	instruction_pointer: u64,

	/// Flags
	rflags: u64,
}

impl ProcessorState {
	pub fn new(memory: MemoryManagementUnit, devices: PortDevices) -> ProcessorState {
		ProcessorState {
			registers: Registers::new(),
			memory,
			devices,
			cpl: 0,
			interupt_stack_pointer: 0,
			idt: 0,
			instruction_pointer: 0,
			rflags: 0,
		}
	}

	fn interrupt(&mut self, interrupt: Interrupt) {
		info(&format!(
			"Rip: 0x{:X}, Interrupt: {interrupt}",
			self.instruction_pointer
		));
		let (vector, error) = match interrupt {
			Interrupt::Undefined => (0x06, 0x00),
			Interrupt::DoubleFault => (0x08, 0x00),
			Interrupt::GeneralProtection => (0x0D, 0x00),
			Interrupt::PageFault { error_code, cr2 } => {
				self.registers.cr2 = cr2;
				(0x0E, error_code)
			}
		};
		let interrupt_entry_ptr = self.idt + 16 * vector;
		if try {
			let data: [u8; 16] =
				std::array::try_from_fn(|i| self.memory.read_u8(interrupt_entry_ptr + i as u64))?;
			let entry: InteruptDescriptorEntry = unsafe { std::mem::transmute(data) };
			if !entry.present || entry.rpl < self.cpl {
				Err(Interrupt::DoubleFault)?;
			}
			let stack_pointer = self.registers.primary_registers[4];
			let new_stack_pointer = if self.cpl <= 0 {
				stack_pointer
			} else {
				self.interupt_stack_pointer
			};
			self.memory
				.write_u64(new_stack_pointer - 8, stack_pointer)?;
			self.memory.write_u64(
				new_stack_pointer - 16,
				((self.cpl as i64 as u64) << 32) | self.rflags,
			)?;
			self.memory
				.write_u64(new_stack_pointer - 24, self.instruction_pointer)?;
			self.memory
				.write_u64(new_stack_pointer - 32, error as u64)?;
			self.instruction_pointer = entry.service_routine;
			self.registers.primary_registers[4] = new_stack_pointer - 32;
			self.cpl = 0;
		}
		.is_err()
		{
			if matches!(interrupt, Interrupt::DoubleFault) {
				fatal("Tripple fault");
			} else {
				self.interrupt(Interrupt::DoubleFault);
			}
		}
	}

	fn write_rm(
		&mut self,
		rm: Rm,
		displacement: u64,
		value: u64,
		bytes: usize,
	) -> Result<(), Interrupt> {
		let value_bytes = value.to_le_bytes();
		let address = match rm {
			Rm::Reg(r) => {
				let mut mask = 0;
				for i in 0..bytes {
					mask |= 0xFF << (i * 8);
				}
				self.registers.primary_registers[r as usize] ^=
					(self.registers.primary_registers[r as usize] & mask) ^ (value & mask);
				return Ok(());
			}
			Rm::Mem(r) => self.registers.primary_registers[r as usize] + displacement,
			Rm::Sib { scale, index, base } => {
				let base = match base {
					0xFE => self.instruction_pointer,
					0xFF => 0,
					_ => self.registers.primary_registers[base as usize],
				};
				let index = match index {
					0x04 => 0,
					_ => self.registers.primary_registers[index as usize],
				};
				(index << scale) + base + displacement
			}
			Rm::RipRel => self.instruction_pointer + displacement,
		};
		for (value, address) in value_bytes.into_iter().zip(address..).take(bytes) {
			self.memory.write_u8(address, value)?;
		}
		Ok(())
	}

	fn read_rm(&mut self, rm: Rm, displacement: u64, bytes: usize) -> Result<u64, Interrupt> {
		let address = match rm {
			Rm::Reg(r) => {
				let mut mask = 0;
				for i in 0..bytes {
					mask |= 0xFF << (i * 8);
				}
				return Ok(self.registers.primary_registers[r as usize] & mask);
			}
			Rm::Mem(r) => self.registers.primary_registers[r as usize] + displacement,
			Rm::Sib { scale, index, base } => {
				let base = match base {
					0xFE => self.instruction_pointer,
					0xFF => 0,
					_ => self.registers.primary_registers[base as usize],
				};
				let index = match index {
					0x04 => 0,
					_ => self.registers.primary_registers[index as usize],
				};
				(index << scale) + base + displacement
			}
			Rm::RipRel => self.instruction_pointer + displacement,
		};
		let mut ret = [0; 8];
		for (ret, address) in ret.iter_mut().zip(address..).take(bytes) {
			*ret = self.memory.read_u8(address)?;
		}
		Ok(u64::from_le_bytes(ret))
	}

	/// Steps one instruction execution
	pub fn step_instruction(&mut self) {
		if let Err(interrupt) = try {
			let (instruction, size) = decode(&mut self.memory, self.instruction_pointer)?;
			match instruction {
				Instruction::In8 { imm } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					self.registers.primary_registers[0] = (self.registers.primary_registers[0]
						& 0xFFFF_FFFF_FFFF_FF00)
						| self.devices.in_u8(imm as u16) as u64;
				}
				Instruction::IncReg { register } => match register.size {
					RegisterSize::_8L => todo!(),
					RegisterSize::_8H => todo!(),
					RegisterSize::_16 => todo!(),
					RegisterSize::_32 => {
						let reg = &mut self.registers.primary_registers[register.selector as usize];
						*reg = (*reg as u32).wrapping_add(1) as u64;
					}
					RegisterSize::_64 => todo!(),
				},
				Instruction::Iret => {
					let instruction_pointer = self
						.memory
						.read_u64(self.registers.primary_registers[4] + 8)?;
					let rflags = self
						.memory
						.read_u64(self.registers.primary_registers[4] + 16)?;
					let stack_pointer = self
						.memory
						.read_u64(self.registers.primary_registers[4] + 24)?;
					self.instruction_pointer = instruction_pointer;
					self.rflags = rflags & 0xFFFF_FFFF;
					self.registers.primary_registers[4] = stack_pointer;
					self.cpl = ((rflags as i64) >> 32) as i8;
					return; // Skip incrementing the instruction pointer as
					// this changes the instruction pointer as part of
					// the instruction.
				}
				Instruction::JmpRel8 { rel } => {
					self.instruction_pointer = self
						.instruction_pointer
						.wrapping_add(rel as i64 as u64) // Sign extend
						.wrapping_add(size);
					return;
				}
				Instruction::MovReg32Imm { register, imm } => {
					self.registers.primary_registers[register as usize] = imm as u64;
				}
				Instruction::MovReg32RM32 {
					dest,
					src,
					displacment,
				} => {
					let value = self.read_rm(src, displacment, 4)?;
					self.registers.primary_registers[dest as usize] = value;
				}
				Instruction::MovReg64Imm { register, imm } => {
					self.registers.primary_registers[register as usize] = imm;
				}
				Instruction::MovRM32Reg32 {
					dest,
					src,
					displacment,
				} => {
					let value = self.registers.primary_registers[src as usize] & 0xFFFF_FFFF;
					self.write_rm(dest, displacment, value, 4)?;
				}
				Instruction::Out8 { imm } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					self.devices
						.out_u8(imm as u16, self.registers.primary_registers[0] as u8);
				}
			}
			self.instruction_pointer = self.instruction_pointer.wrapping_add(size);
		} {
			self.interrupt(interrupt);
		}
	}

	pub fn eprint_primary_registers(&self) {
		eprintln!("rax: {}", self.registers.primary_registers[0]);
		eprintln!("rbx: {}", self.registers.primary_registers[3]);
		eprintln!("rcx: {}", self.registers.primary_registers[1]);
		eprintln!("rdx: {}", self.registers.primary_registers[2]);
		eprintln!("rdi: {}", self.registers.primary_registers[7]);
		eprintln!("rsi: {}", self.registers.primary_registers[6]);
		eprintln!("rbp: {}", self.registers.primary_registers[5]);
		eprintln!("rsp: {}", self.registers.primary_registers[4]);
	}
}
