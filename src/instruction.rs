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
pub enum Instruction {
	In8 { imm: i8 },
	IncReg { register: Register },
	Iret,
	JmpRel8 { rel: i8 },
	MovReg32Imm { register: u8, imm: u32 },
	MovReg64Imm { register: u8, imm: u64 },
	Out8 { imm: i8 },
}
