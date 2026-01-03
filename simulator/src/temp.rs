use crate::{interupt::Interrupt, memory::MemoryManagementUnit};

enum LockRep {
	Lock,
	Rep,
	Repe,
	Repne,
}

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

enum SegmentOverride {
	None,
	Fs,
	Gs,
}

enum RM {
	Reg(u8),
	RipRel {
		displacement: u64,

		/// If this flag is set the address should truncated to 32 bits.
		address_override: bool,
	},
	Mem {
		index: u8,
		scale: u8,
		base: u8,
		displacement: u64,

		/// If this flag is set the address should truncated to 32 bits.
		address_override: bool,

		/// Add fsbase or gsbase.
		segment_override: SegmentOverride,
	},
}

struct Reg(u8);

struct Immediate(u64);

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

fn wide(rex: Option<Rex>) -> bool {
	match rex {
		Some(rex) => rex.w,
		None => false,
	}
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
