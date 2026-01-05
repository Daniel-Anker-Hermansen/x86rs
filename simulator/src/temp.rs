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

enum SegmentOverride {
	None,
	Fs,
	Gs,
}

enum RM {
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

struct Reg(u8);

impl Reg {
	fn parse_suffix(opcode: u8, rex: Option<Rex>) -> Reg {
		Reg(((rex_b(rex) as u8) << 3) | (opcode & 0x07))
	}
}

struct Immediate(u64);

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
				*size += 1;
				let sib_byte = mmu.read_u8(instruction_pointer + *size - 1)?;
				let mut displacement_bytes = [0; 4];
				if sib_byte & 7 == 5 {
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
					parse_sib(
						sib_byte,
						address_override,
						segment_override,
						u32::from_le_bytes(displacement_bytes),
						rex,
					)
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
			let sib_byte = mmu.read_u8(instruction_pointer + *size)?;
			*size += 1;
			let displacement = mmu.read_u8(instruction_pointer + *size)? as u32;
			*size += 1;
			if rm_field == 4 {
				*size += 1;
				parse_sib(
					sib_byte,
					address_override,
					segment_override,
					displacement,
					rex,
				)
			} else {
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
			let sib_byte = mmu.read_u8(instruction_pointer + *size)?;
			*size += 1;
			let mut displacement_bytes = [0; 4];
			for i in 0..4 {
				displacement_bytes[i] = mmu.read_u8(instruction_pointer + *size)?;
				*size += 1;
			}
			let displacement = u32::from_le_bytes(displacement_bytes);
			if rm_field == 4 {
				parse_sib(
					sib_byte,
					address_override,
					segment_override,
					displacement,
					rex,
				)
			} else {
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
	In16 E5 Imm16 : so;
	In32 E5 Imm32 :;
	In8D EC :;
	In16D ED : so;
	In32D ED :;
	MovReg8Imm B0 SR Imm8 :;
	MovReg16Imm B8 SR Imm16 : so;
	MovReg32Imm B8 SR Imm32 :;
	MovReg64Imm B8 SR Imm64 : w;
	Swi4 3F01 RM :;
	Wrcr 3F00 Imm8 RM :;
);
