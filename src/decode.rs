use crate::{
	instruction::{Instruction, Register, RegisterSize},
	interupt::Interrupt,
	memory::MemoryManagementUnit,
};

#[derive(Clone, Copy, Debug)]
enum Prefix1 {
	Lock,
	Repne,
	Repe,
}

#[derive(Clone, Copy, Debug)]
enum Prefix2 {
	SSOverride,
	ESOverride,
	FSOverride,
	GSOverride,
	BranchNotTaken,
	BranchTaken,
}

#[derive(Clone, Copy, Debug)]
struct Prefix3;

#[derive(Clone, Copy, Debug)]
struct Prefix4;

#[derive(Clone, Copy, Debug)]
struct Rex {
	w: bool,
	r: bool,
	x: bool,
	b: bool,
}

#[derive(Clone, Copy, Debug)]
enum Opcode {
	One(u8),
	Two(u8),
	Three38(u8),
	Three3A(u8),
}

#[derive(Clone, Copy, Debug)]
struct ModRM {
	r#mod: u8,
	reg: u8,
	rm: u8,
}

impl ModRM {
	fn decode(byte: u8) -> ModRM {
		ModRM {
			r#mod: (byte >> 6) & 0x03,
			reg: (byte >> 3) & 0x07,
			rm: byte & 0x07,
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct Sib {
	scale: u8,
	index: u8,
	base: u8,
}

#[derive(Clone, Copy, Debug)]
struct InstructionParts {
	prefix_1: Option<Prefix1>,
	prefix_2: Option<Prefix2>,
	prefix_3: Option<Prefix3>,
	prefix_4: Option<Prefix4>,
	rex: Option<Rex>,
	opcode: Option<Opcode>,
	modrm: Option<ModRM>,
	sib: Option<Sib>,
	displacement: u64,
	immediate: u64,
}

impl InstructionParts {
	fn nedds_modrm(&self) -> bool {
		match self.opcode.expect("only called after opcode") {
			Opcode::One(byte) => match byte {
				0xFF => true,
				_ => false,
			},
			Opcode::Two(_) => false,
			Opcode::Three38(_) => false,
			Opcode::Three3A(_) => false,
		}
	}

	fn displacement_size(&self) -> usize {
		match self.opcode.expect("only called after opcode") {
			Opcode::One(byte) => match byte {
				0xEB => 1,
				_ => 0,
			},
			Opcode::Two(_) => 0,
			Opcode::Three38(_) => 0,
			Opcode::Three3A(_) => 0,
		}
	}

	fn wide(&self) -> bool {
		self.rex.map(|rex| rex.w).unwrap_or(false)
	}

	fn rex_r(&self) -> u8 {
		self.rex.map(|rex| (rex.r as u8) << 3).unwrap_or(0)
	}

	fn rex_b(&self) -> u8 {
		self.rex.map(|rex| (rex.b as u8) << 3).unwrap_or(0)
	}

	fn immediate_size(&self) -> usize {
		match self.opcode.expect("only called after opcode") {
			Opcode::One(byte) => match byte {
				0xB8..0xC0 => {
					if self.wide() {
						8
					} else {
						4
					}
				}
				0xE4 => 1,
				0xE6 => 1,
				_ => 0,
			},
			Opcode::Two(_) => 0,
			Opcode::Three38(_) => 0,
			Opcode::Three3A(_) => 0,
		}
	}

	fn into_instruction(self) -> Result<Instruction, Interrupt> {
		match self.opcode.expect("only called after opcode") {
			Opcode::One(byte) => match byte {
				0xB8..0xC0 if self.wide() => Ok(Instruction::MovReg64Imm {
					register: (byte & 0x07) | self.rex_b(),
					imm: self.immediate,
				}),
				0xB8..0xC0 => Ok(Instruction::MovReg32Imm {
					register: (byte & 0x07) | self.rex_b(),
					imm: self.immediate as u32,
				}),
				0xCF => Ok(Instruction::Iret), // Always 64-bit. Ignore REX prefix.
				0xE4 => Ok(Instruction::In8 {
					imm: self.immediate as i8,
				}),
				0xE6 => Ok(Instruction::Out8 {
					imm: self.immediate as i8,
				}),
				0xEB => Ok(Instruction::JmpRel8 {
					rel: self.displacement as i8,
				}),
				0xFF => {
					let modrm = self.modrm.expect("opcode requires modrm");
					match modrm.reg {
						0x00 => {
							let reg = Register {
								selector: modrm.rm,
								size: RegisterSize::_32,
							};
							match modrm.r#mod {
								0x3 => Ok(Instruction::IncReg { register: reg }),
								_ => Err(Interrupt::Undefined),
							}
						}
						_ => Err(Interrupt::Undefined),
					}
				}
				_ => Err(Interrupt::Undefined),
			},
			Opcode::Two(_) => Err(Interrupt::Undefined),
			Opcode::Three38(_) => Err(Interrupt::Undefined),
			Opcode::Three3A(_) => Err(Interrupt::Undefined),
		}
	}
}

pub fn decode(
	memory: &mut MemoryManagementUnit,
	rip: u64,
) -> Result<(Instruction, u64), Interrupt> {
	let mut instruction = InstructionParts {
		prefix_1: None,
		prefix_2: None,
		prefix_3: None,
		prefix_4: None,
		rex: None,
		opcode: None,
		modrm: None,
		sib: None,
		displacement: 0,
		immediate: 0,
	};
	let mut size = 0;
	while instruction.opcode.is_none() {
		let byte = memory.read_u8(rip + size)?;
		dbg!(byte);
		size += 1;
		match byte {
			0xF0
			| 0xF2
			| 0xF3
			| 0x36
			| 0x26
			| 0x64
			| 0x65
			| 0x2E
			| 0x3E
			| 0x66
			| 0x67
			| 0x40..0x50 => {
				instruction.rex = None; // A rex not immediately preceding an
				// opcode is ignored.
				match byte {
					0xF0 => instruction.prefix_1 = Some(Prefix1::Lock),
					0xF2 => instruction.prefix_1 = Some(Prefix1::Repne),
					0xF3 => instruction.prefix_1 = Some(Prefix1::Repe),
					0x36 => instruction.prefix_2 = Some(Prefix2::SSOverride),
					0x26 => instruction.prefix_2 = Some(Prefix2::ESOverride),
					0x64 => instruction.prefix_2 = Some(Prefix2::FSOverride),
					0x65 => instruction.prefix_2 = Some(Prefix2::GSOverride),
					0x2E => instruction.prefix_2 = Some(Prefix2::BranchTaken),
					0x3E => instruction.prefix_2 = Some(Prefix2::BranchNotTaken),
					0x66 => instruction.prefix_3 = Some(Prefix3),
					0x67 => instruction.prefix_4 = Some(Prefix4),
					0x40..0x50 => {
						instruction.rex = Some(Rex {
							w: (byte >> 3) & 1 == 1,
							r: (byte >> 2) & 1 == 1,
							x: (byte >> 1) & 1 == 1,
							b: byte & 1 == 1,
						})
					}
					_ => unreachable!(),
				}
			}
			0x0F => {
				let sbyte = memory.read_u8(rip + size)?;
				size += 1;
				match sbyte {
					0x38 => {
						let tbyte = memory.read_u8(rip + size)?;
						size += 1;
						instruction.opcode = Some(Opcode::Three38(tbyte));
					}
					0x3A => {
						let tbyte = memory.read_u8(rip + size)?;
						size += 1;
						instruction.opcode = Some(Opcode::Three3A(tbyte));
					}
					_ => instruction.opcode = Some(Opcode::Two(sbyte)),
				}
			}
			_ => instruction.opcode = Some(Opcode::One(byte)),
		}
	}
	if instruction.nedds_modrm() {
		instruction.modrm = Some(ModRM::decode(memory.read_u8(rip + size)?));
		size += 1;
	}
	for i in 0..instruction.displacement_size() {
		let byte = memory.read_u8(rip + size)?;
		size += 1;
		instruction.displacement |= (byte as u64) << (8 * i);
	}
	for i in 0..instruction.immediate_size() {
		let byte = memory.read_u8(rip + size)?;
		size += 1;
		instruction.immediate |= (byte as u64) << (8 * i);
	}
	Ok((instruction.into_instruction()?, size))
}

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
		dbg!(
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
				.unwrap()
				.code()
		);
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
