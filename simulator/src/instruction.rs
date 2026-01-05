use crate::{interupt::Interrupt, memory::MemoryManagementUnit};

enum LockRep {
	Lock,
	Rep,
	Repe,
	Repne,
}

#[derive(Clone, Copy)]
struct Rex {
	w: bool,
	r: bool,
	x: bool,
	b: bool,
}

impl Rex {
	fn new(byte: u8) -> Rex {
		Rex {
			w: (byte >> 3) & 1 == 1,
			r: (byte >> 2) & 1 == 1,
			x: (byte >> 1) & 1 == 1,
			b: byte & 1 == 1,
		}
	}
}

fn rex_w(rex: Option<Rex>) -> bool {
	match rex {
		Some(rex) => rex.w,
		None => false,
	}
}

fn rex_r(rex: Option<Rex>) -> bool {
	match rex {
		Some(rex) => rex.r,
		None => false,
	}
}

fn rex_x(rex: Option<Rex>) -> bool {
	match rex {
		Some(rex) => rex.x,
		None => false,
	}
}

fn rex_b(rex: Option<Rex>) -> bool {
	match rex {
		Some(rex) => rex.b,
		None => false,
	}
}

#[derive(Clone, Copy, Debug)]
pub enum SegmentOverride {
	None,
	Fs,
	Gs,
}

#[derive(Clone, Copy, Debug)]
pub enum RM {
	Reg(u8),
	RipRel {
		displacement: u32,

		/// If this flag is set the address should truncated to 32 bits.
		address_override: bool,
	},
	Mem {
		index: u8,
		scale: u8,
		base: u8,
		displacement: u32,

		/// If this flag is set the address should truncated to 32 bits.
		address_override: bool,

		/// Add fsbase or gsbase.
		segment_override: SegmentOverride,
	},
}

#[derive(Clone, Copy, Debug)]
pub struct Reg(pub u8);

impl Reg {
	fn parse_suffix(opcode: u8, rex: Option<Rex>) -> Reg {
		Reg(((rex_b(rex) as u8) << 3) | (opcode & 0x07))
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Immediate(pub u64);

impl Immediate {
	fn parse(immediate: u64) -> Immediate {
		Immediate(immediate)
	}
}

// TODO: Handle byte registers without rex predix.
fn parse_sib(
	byte: u8,
	address_override: bool,
	segment_override: SegmentOverride,
	displacement: u32,
	rex: Option<Rex>,
) -> RM {
	let scale = (byte >> 6) & 3;
	let index = ((rex_x(rex) as u8) << 3) | ((byte >> 3) & 7);
	let base = ((rex_b(rex) as u8) << 3) | (byte & 7);
	RM::Mem {
		index,
		scale,
		base,
		displacement,
		address_override,
		segment_override,
	}
}

fn parse_sib_no_base(
	byte: u8,
	address_override: bool,
	segment_override: SegmentOverride,
	displacement: u32,
	rex: Option<Rex>,
) -> RM {
	let scale = (byte >> 6) & 3;
	let index = ((rex_x(rex) as u8) << 3) | ((byte >> 3) & 7);
	let base = 0xFF; // We set the base to 0xFF to indicate that is zero.
	RM::Mem {
		index,
		scale,
		base,
		displacement,
		address_override,
		segment_override,
	}
}

fn read_modrm(
	mmu: &mut MemoryManagementUnit,
	size: &mut u64,
	instruction_pointer: u64,
	address_override: bool,
	segment_override: SegmentOverride,
	rex: Option<Rex>,
) -> Result<(u8, RM), Interrupt> {
	let modrm_byte = mmu.read_u8(instruction_pointer + *size)?;
	*size += 1;
	let reg = (modrm_byte >> 3) & 0x7;
	let rm_field = modrm_byte & 0x7;
	let rm = match modrm_byte >> 6 {
		0x00 => match rm_field {
			4 => {
				let sib_byte = mmu.read_u8(instruction_pointer + *size)?;
				*size += 1;
				if sib_byte & 7 == 5 {
					let mut displacement_bytes = [0; 4];
					for i in 0..4 {
						displacement_bytes[i] = mmu.read_u8(instruction_pointer + *size)?;
						*size += 1;
					}
					parse_sib_no_base(
						sib_byte,
						address_override,
						segment_override,
						u32::from_le_bytes(displacement_bytes),
						rex,
					)
				} else {
					parse_sib(sib_byte, address_override, segment_override, 0, rex)
				}
			}
			5 => {
				let mut displacement_bytes = [0; 4];
				for i in 0..4 {
					displacement_bytes[i] = mmu.read_u8(instruction_pointer + *size)?;
					*size += 1;
				}
				RM::RipRel {
					displacement: u32::from_le_bytes(displacement_bytes),
					address_override,
				}
			}
			_ => RM::Mem {
				index: 4,
				scale: 0,
				base: ((rex_b(rex) as u8) << 3) | rm_field,
				displacement: 0,
				address_override,
				segment_override,
			},
		},
		0x01 => {
			if rm_field == 4 {
				let sib_byte = mmu.read_u8(instruction_pointer + *size)?;
				*size += 1;
				let displacement = mmu.read_u8(instruction_pointer + *size)? as u32;
				*size += 1;
				parse_sib(
					sib_byte,
					address_override,
					segment_override,
					displacement,
					rex,
				)
			} else {
				let displacement = mmu.read_u8(instruction_pointer + *size)? as u32;
				*size += 1;
				RM::Mem {
					index: 4,
					scale: 0,
					base: ((rex_b(rex) as u8) << 3) | rm_field,
					displacement,
					address_override,
					segment_override,
				}
			}
		}
		0x02 => {
			if rm_field == 4 {
				let sib_byte = mmu.read_u8(instruction_pointer + *size)?;
				*size += 1;
				let mut displacement_bytes = [0; 4];
				for i in 0..4 {
					displacement_bytes[i] = mmu.read_u8(instruction_pointer + *size)?;
					*size += 1;
				}
				let displacement = u32::from_le_bytes(displacement_bytes);
				parse_sib(
					sib_byte,
					address_override,
					segment_override,
					displacement,
					rex,
				)
			} else {
				let mut displacement_bytes = [0; 4];
				for i in 0..4 {
					displacement_bytes[i] = mmu.read_u8(instruction_pointer + *size)?;
					*size += 1;
				}
				let displacement = u32::from_le_bytes(displacement_bytes);
				RM::Mem {
					index: 4,
					scale: 0,
					base: ((rex_b(rex) as u8) << 3) | rm_field,
					displacement,
					address_override,
					segment_override,
				}
			}
		}
		0x03 => RM::Reg(((rex_b(rex) as u8) << 3) | rm_field),
		_ => unreachable!(),
	};
	Ok((((rex_r(rex) as u8) << 3) | reg, rm))
}

fn read_immediate(
	mmu: &mut MemoryManagementUnit,
	size: &mut u64,
	instruction_pointer: u64,
	nbytes: u8,
) -> Result<u64, Interrupt> {
	let mut bytes = [0; 8];
	for i in 0..nbytes {
		bytes[i as usize] = mmu.read_u8(instruction_pointer + *size)?;
		*size += 1;
	}
	Ok(u64::from_le_bytes(bytes))
}

// so: Size override prefix
// w: REX.w
simulator_macros::generate_instructions!(
	In8 E4 Imm8 :;
	In16 E5 Imm8 : so;
	In32 E5 Imm8 :;
	In8D EC :;
	In16D ED : so;
	In32D ED :;
	IncRM8 FE00 RM :;
	IncRM16 FF00 RM : so;
	IncRM32 FF00 RM :;
	IncRM64 FF00 RM : w;
	Iret CF :;
	JmpRel8 EB Imm8 :;
	JmpRel32 E9 Imm32 :;
	MovReg8Imm B0 SR Imm8 :;
	MovReg16Imm B8 SR Imm16 : so;
	MovReg32Imm B8 SR Imm32 :;
	MovReg64Imm B8 SR Imm64 : w;
	MovReg8RM 8A R RM :;
	MovReg16RM 8B R RM : so;
	MovReg32RM 8B R RM :;
	MovReg64RM 8B R RM : w;
	MovRM8Reg 88 RM R :;
	MovRM16Reg 89 RM R : so;
	MovRM32Reg 89 RM R :;
	MovRM64Reg 89 RM R : w;
	Out8 E6 Imm8 :;
	Out16 E7 Imm8 : so;
	Out32 E7 Imm8 :;
	Swi4 3F01 RM :;
	Wrcr 3F00 Imm8 RM :;
);

#[cfg(test)]
mod test {
	use std::process::Command;

	use crate::{
		decode::decode,
		instruction::Instruction,
		memory::{MemoryManagementUnit, PhysicalMemoryManagementUnit, ReadOnlyMemory},
	};

	fn test_instruction(data: &[u8], expected: Instruction) {
		let mut pmu = PhysicalMemoryManagementUnit::new();
		let mut rom = vec![0; (4 << 12) + data.len()];
		rom[0..8].copy_from_slice(&0x0000_0000_0000_1001u64.to_le_bytes());
		rom[1 << 12..(1 << 12) + 8].copy_from_slice(&0x0000_0000_0000_2001u64.to_le_bytes());
		rom[2 << 12..(2 << 12) + 8].copy_from_slice(&0x0000_0000_0000_3001u64.to_le_bytes());
		rom[3 << 12..(3 << 12) + 8].copy_from_slice(&0x0000_0000_0000_4001u64.to_le_bytes());
		rom[4 << 12..].copy_from_slice(data);
		pmu.add(0, rom.len() as u64, || {
			ReadOnlyMemory::create(&rom, rom.len() as u64)
		});
		let mut mmu = MemoryManagementUnit::new(pmu);
		let (instruction, size) = decode(&mut mmu, 0).unwrap();
		assert_eq!(size, data.len() as u64);
		assert_eq!(instruction, expected);
	}

	fn test_nasm(instruction: &str, expected: Instruction) {
		let file_name = format!("__{}", instruction.split_whitespace().collect::<String>());
		std::fs::write(
			format!("{file_name}.s"),
			format!("[bits 64]\n{instruction}"),
		)
		.unwrap();
		Command::new("nasm")
			.args([
				&format!("{file_name}.s"),
				"-f",
				"bin",
				"-O0",
				"-o",
				&file_name,
			])
			.spawn()
			.unwrap()
			.wait()
			.unwrap();
		let data = std::fs::read(file_name).unwrap();
		eprintln!("{:x?}", data);
		test_instruction(&data, expected);
	}

	#[test]
	fn mov() {
		test_nasm(
			"mov r15, 0",
			Instruction::MovReg64Imm {
				register: 15,
				imm: 0,
			},
		);
		test_nasm(
			"mov rdx, 9993",
			Instruction::MovReg64Imm {
				register: 2,
				imm: 9993,
			},
		);
	}
}
