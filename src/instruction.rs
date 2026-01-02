#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterSize {
	_8L,
	_8H,
	_16,
	_32,
	_64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register {
	pub selector: u8,
	pub size: RegisterSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplacementSize {
	_0,
	_8,
	_32,
}

impl DisplacementSize {
	pub fn into_bytes(self) -> usize {
		match self {
			DisplacementSize::_0 => 0,
			DisplacementSize::_8 => 1,
			DisplacementSize::_32 => 4,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rm {
	Reg(u8),
	Mem(u8),
	Sib { scale: u8, index: u8, base: u8 },
	RipRel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
	In8 {
		imm: i8,
	},
	IncReg {
		register: Register,
	},
	Iret,
	JmpRel8 {
		rel: i8,
	},
	MovReg32Imm {
		register: u8,
		imm: u32,
	},
	MovReg32RM32 {
		dest: u8,
		src: Rm,
		displacment: u64,
	},
	MovReg64Imm {
		register: u8,
		imm: u64,
	},
	MovRM32Reg32 {
		dest: Rm,
		src: u8,
		displacment: u64,
	},
	Out8 {
		imm: i8,
	},
	Swi4 {
		src: Rm,
		displacement: u64,
	},
	Wrcr {
		reg: u8,
		config_reg: u8,
	},
}
