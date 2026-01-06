use std::collections::BTreeMap;

use proc_macro::TokenStream;
use quote::ToTokens;

#[derive(Debug)]
enum OperandEncoding {
	SuffixReg,
	ModReg,
	ModRM,
	Immediate(u8),
	Implicit,
}

impl OperandEncoding {
	fn generate(&self) -> impl ToTokens {
		match self {
			OperandEncoding::SuffixReg => quote::quote! {Reg::parse_suffix(byte, rex)},
			OperandEncoding::ModReg => quote::quote! {Reg(reg)},
			OperandEncoding::ModRM => quote::quote! {rm},
			OperandEncoding::Immediate(_) => quote::quote! {Immediate::parse(immediate)},
			_ => unreachable!(),
		}
	}

	fn operand0(&self) -> Option<impl ToTokens> {
		match self {
			OperandEncoding::Implicit => None,
			_ => {
				let code = self.generate();
				Some(quote::quote! {operand0: #code,})
			}
		}
	}

	fn operand1(&self) -> Option<impl ToTokens> {
		match self {
			OperandEncoding::Implicit => None,
			_ => {
				let code = self.generate();
				Some(quote::quote! {operand1: #code,})
			}
		}
	}
}

#[derive(Debug)]
struct InstructionEncoding {
	/// The name of the instruction
	name: String,

	/// Opcodes. opcode1 and opcode2 are undefined if opcode0 and opcode1 respectively are not
	/// extension opcodes.
	opcode0: u8,
	opcode1: u8,
	opcode2: u8,

	/// Modrm mode is only allowed to be reg (0b11) (Example: inc).
	modrm_only_reg: bool,

	/// Modrm mode is not allowed to be reg (0b11) (Example: lea).
	modrm_only_mem: bool,

	/// Operand encodings.
	operand0: OperandEncoding,
	operand1: OperandEncoding,

	/// Encoding requires size override prefix
	size_override: bool,

	/// Requires REX.w
	wide: bool,
}
impl InstructionEncoding {
	fn suffix_reg(&self) -> bool {
		matches!(self.operand0, OperandEncoding::SuffixReg)
			|| matches!(self.operand1, OperandEncoding::SuffixReg)
	}

	fn needs_modrm(&self) -> bool {
		(matches!(
			self.operand0,
			OperandEncoding::ModRM | OperandEncoding::ModReg
		) || matches!(
			self.operand1,
			OperandEncoding::ModRM | OperandEncoding::ModReg
		)) && (self.opcode0 == 0x0F || self.opcode1 == 0xFF)
	}

	fn immediate_size(&self) -> u8 {
		if let OperandEncoding::Immediate(size) = self.operand0 {
			return size / 8;
		}
		if let OperandEncoding::Immediate(size) = self.operand1 {
			return size / 8;
		}
		0
	}
}

fn parse_operand(src: Option<&str>) -> OperandEncoding {
	match src {
		Some("R") => OperandEncoding::ModReg,
		Some("RM") => OperandEncoding::ModRM,
		Some("SR") => OperandEncoding::SuffixReg,
		Some("Imm8") => OperandEncoding::Immediate(8),
		Some("Imm16") => OperandEncoding::Immediate(16),
		Some("Imm32") => OperandEncoding::Immediate(32),
		Some("Imm64") => OperandEncoding::Immediate(64),
		None => OperandEncoding::Implicit,
		_ => unreachable!(),
	}
}

fn parse_instruction(src: &str) -> InstructionEncoding {
	let (base, modifiers) = src.split_once(":").unwrap();
	let mut tokens = base.split_whitespace();
	let name = tokens.next().unwrap().to_string();
	let opcode = tokens.next().unwrap();
	let opcode0 = opcode
		.get(0..2)
		.map(|x| u8::from_str_radix(x, 16).unwrap())
		.unwrap_or(0);
	let opcode1 = opcode
		.get(2..4)
		.map(|x| u8::from_str_radix(x, 16).unwrap())
		.unwrap_or(0xFF);
	let opcode2 = opcode
		.get(4..6)
		.map(|x| u8::from_str_radix(x, 16).unwrap())
		.unwrap_or(0);
	let operand0 = parse_operand(tokens.next());
	let operand1 = parse_operand(tokens.next());
	let mut instruction = InstructionEncoding {
		name,
		opcode0,
		opcode1,
		opcode2,
		modrm_only_reg: false,
		modrm_only_mem: false,
		operand0,
		operand1,
		size_override: false,
		wide: false,
	};
	for modifier in modifiers.split_whitespace() {
		match modifier {
			"so" => instruction.size_override = true,
			"w" => instruction.wide = true,
			_ => (),
		}
	}
	instruction
}

fn generate_instruction_decode(instruction: &InstructionEncoding) -> impl ToTokens {
	let name = syn::Ident::new(&instruction.name, proc_macro::Span::call_site().into());
	let immediate = instruction.immediate_size();
	let operand0 = instruction.operand0.operand0();
	let operand1 = instruction.operand1.operand1();
	let modrm = instruction.needs_modrm().then(|| quote::quote! {
		let (reg, rm) = read_modrm(mmu, &mut size, instruction_pointer, address_override, segment_override, rex)?;
	});
	quote::quote! {
		#modrm
		let immediate = read_immediate(mmu, &mut size, instruction_pointer, #immediate)?;
		return Ok((Instruction:: #name {#operand0 #operand1}, size));
	}
}

fn generate_opcode_arm(instructions: Vec<&InstructionEncoding>, reg_opcode: bool) -> impl ToTokens {
	assert!(instructions[0].opcode0 != 0x0F);

	if instructions[0].opcode1 != 0xFF && !reg_opcode {
		// This means that reg field is used as an opcode extension
		let mut groups = BTreeMap::<u8, Vec<&InstructionEncoding>>::new();

		for instruction in &instructions {
			groups
				.entry(instruction.opcode1)
				.or_default()
				.push(instruction);
		}

		let arms = groups.into_iter().map(|(code, instructions)| {
			let handler = generate_opcode_arm(instructions, true);
			quote::quote! {#code => #handler, }
		});

		return quote::quote! {{
			let (reg, rm) = read_modrm(mmu, &mut size, instruction_pointer, address_override, segment_override, rex)?;
			match reg {
				#(#arms)*
				_ => Err(Interrupt::Undefined),
			}
		}};
	}

	let names = instructions.iter().map(|x| &x.name);

	let wide_instruction = instructions
		.iter()
		.find(|instruction| instruction.wide)
		.map(|instruction| {
			let instruction = generate_instruction_decode(instruction);
			quote::quote! {
				if rex_w(rex) {
					#instruction
				}
			}
		});

	let so_instruction = instructions
		.iter()
		.find(|instruction| instruction.size_override)
		.map(|instruction| {
			let instruction = generate_instruction_decode(instruction);
			quote::quote! {
				if size_override {
					#instruction
				}
			}
		});

	let default = instructions
		.iter()
		.find(|instruction| !instruction.size_override && !instruction.wide)
		.map(|instruction| {
			let instruction = generate_instruction_decode(instruction);
			quote::quote! { #instruction }
		})
		.unwrap_or_else(|| quote::quote! { Err(Interrupt::Undefined) });

	quote::quote! {{
		let x = [#(#names), *];
		#wide_instruction
		#so_instruction
		#default
	}}
}

#[proc_macro]
pub fn generate_instructions(tokens: TokenStream) -> TokenStream {
	let src = tokens.to_string();
	let instructions: Vec<InstructionEncoding> = src
		.split(";")
		.filter(|x| !x.is_empty())
		.map(parse_instruction)
		.collect();

	let enum_variants: Vec<_> = instructions
		.iter()
		.map(|x| {
			let name = syn::Ident::new(&x.name, proc_macro::Span::call_site().into());
			let operand0 = match x.operand0 {
				OperandEncoding::SuffixReg | OperandEncoding::ModReg => {
					quote::quote! {operand0: Reg,}
				}
				OperandEncoding::ModRM => quote::quote! {operand0: RM,},
				OperandEncoding::Immediate(_) => quote::quote! {operand0: Immediate,},
				OperandEncoding::Implicit => quote::quote! {},
			};
			let operand1 = match x.operand1 {
				OperandEncoding::SuffixReg | OperandEncoding::ModReg => {
					quote::quote! {operand1: Reg,}
				}
				OperandEncoding::ModRM => quote::quote! {operand1: RM,},
				OperandEncoding::Immediate(_) => quote::quote! {operand1: Immediate,},
				OperandEncoding::Implicit => quote::quote! {},
			};
			quote::quote! {#name {#operand0 #operand1},}
		})
		.collect();

	let instruction_definition =
		quote::quote! { #[derive(Debug, Eq, PartialEq)] pub enum Instruction {#(#enum_variants)*}};

	let decode_function = quote::quote! {
		pub fn decode(mmu: &mut MemoryManagementUnit, instruction_pointer: u64) -> Result<(Instruction, u64), Interrupt> {
			decode_internal(mmu, instruction_pointer, false, false, None, SegmentOverride::None, None)
		}
	};

	let mut groups = BTreeMap::<u8, Vec<&InstructionEncoding>>::new();

	for instruction in &instructions {
		if instruction.suffix_reg() {
			for opcode0 in instruction.opcode0..instruction.opcode0 + 8 {
				groups.entry(opcode0).or_default().push(instruction);
			}
		} else {
			groups
				.entry(instruction.opcode0)
				.or_default()
				.push(instruction);
		}
	}

	let opcode1 = groups.into_iter().map(|(code, instructions)| {
		let handler = generate_opcode_arm(instructions, false);
		quote::quote! {#code => #handler, }
	});

	let decode_internal_function = quote::quote! {
		fn decode_internal(mmu: &mut MemoryManagementUnit, instruction_pointer: u64, size_override: bool, address_override: bool, lock_rep: Option<LockRep>, segment_override: SegmentOverride, rex: Option<Rex>) -> Result<(Instruction, u64), Interrupt> {
			let byte = mmu.read_u8(instruction_pointer)?;
			let mut size = 1;
			match byte {
				0x26 | 0x2E | 0x36 | 0x3E => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, lock_rep, segment_override, None)?;
					return Ok((instruction, size + 1));
				}
				0x64 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, lock_rep, SegmentOverride::Fs, None)?;
					return Ok((instruction, size + 1));
				}
				0x65 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, lock_rep, SegmentOverride::Gs, None)?;
					return Ok((instruction, size + 1));
				}
				0x66 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, true, address_override, lock_rep, segment_override, None)?;
					return Ok((instruction, size + 1));
				}
				0x67 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, true, lock_rep, segment_override, None)?;
					return Ok((instruction, size + 1));
				}
				0xF0 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, Some(LockRep::Lock), segment_override, None)?;
					return Ok((instruction, size + 1));
				},
				0xF2 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, Some(LockRep::Repne), segment_override, None)?;
					return Ok((instruction, size + 1));
				},
				0xF3 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, Some(LockRep::Repe), segment_override, None)?;
					return Ok((instruction, size + 1));
				},
				0x40..0x50 => {
					let (instruction, size) = decode_internal(mmu, instruction_pointer + 1, size_override, address_override, lock_rep, segment_override, Some(Rex::new(byte)))?;
					return Ok((instruction, size + 1));
				}
				_ => (),
			}
			match byte {
				#(#opcode1)*
				_ => Err(Interrupt::Undefined),
			}
		}
	};

	quote::quote! {
		#instruction_definition

		#decode_function

		#decode_internal_function
	}
	.into()
}
