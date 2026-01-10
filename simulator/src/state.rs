use std::sync::atomic::{AtomicU8, Ordering};

use crate::{
	device::PortDevices,
	error::{fatal, info},
	instruction::{Instruction, RM, Reg, decode},
	interupt::{Interrupt, InteruptDescriptorEntry},
	memory::MemoryManagementUnit,
};

const A: Reg = Reg(0);
const SP: Reg = Reg(4);

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

static IRQ: AtomicU8 = AtomicU8::new(0);

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

macro_rules! read_write_rm {
	($size:ident) => {
		fn ${concat(write_rm_, $size)}(&mut self, rm: RM, value: $size) -> Result<(), Interrupt> {
			match rm {
				RM::Reg(reg) => Ok(self.${concat(write_reg_, $size)}(Reg(reg), value)),
				RM::RipRel {
					displacement,
					address_override,
				} => {
					let rip = if address_override {
						self.instruction_pointer & 0xFFFF
					} else {
						self.instruction_pointer
					};
					self.memory.${concat(write_, $size)}(rip + displacement as u64, value)
				}
				RM::Mem {
					index,
					scale,
					base,
					displacement,
					address_override,
					#[allow(unused)]
					segment_override,
				} => {
					let base = if base == 0xFF {
						0
					} else {
						self.read_reg_u64(Reg(base))
					};
					let index = if index == 4 {
						0
					} else {
						self.read_reg_u64(Reg(index))
					};
					let address = base + (index << scale) + displacement as u64;
					let address = if address_override {
						address & 0xFFFF
					} else {
						address
					};
					self.memory.${concat(write_, $size)}(address, value)
				}
			}
		}

		fn ${concat(read_rm_, $size)}(&mut self, rm: RM) -> Result<$size, Interrupt> {
			match rm {
				RM::Reg(reg) => Ok(self.${concat(read_reg_, $size)}(Reg(reg))),
				RM::RipRel {
					displacement,
					address_override,
				} => {
					let rip = if address_override {
						self.instruction_pointer & 0xFFFF
					} else {
						self.instruction_pointer
					};
					self.memory.${concat(read_, $size)}(rip + displacement as u64)
				}
				RM::Mem {
					index,
					scale,
					base,
					displacement,
					address_override,
					#[allow(unused)]
					segment_override,
				} => {
					let base = if base == 0xFF {
						0
					} else {
						self.read_reg_u64(Reg(base))
					};
					let index = if index == 4 {
						0
					} else {
						self.read_reg_u64(Reg(index))
					};
					let address = base + (index << scale) + displacement as u64;
					let address = if address_override {
						address & 0xFFFF
					} else {
						address
					};
					self.memory.${concat(read_, $size)}(address)
				}
			}
		}
	};
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
			Interrupt::IRQ(irq) => (irq as u64, 0x00),
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

	fn write_reg_u8(&mut self, Reg(reg): Reg, value: u8) {
		let handle = &mut self.registers.primary_registers[reg as usize];
		*handle ^= (*handle & 0xFF) ^ value as u64;
	}

	fn write_reg_u16(&mut self, Reg(reg): Reg, value: u16) {
		let handle = &mut self.registers.primary_registers[reg as usize];
		*handle ^= (*handle & 0xFF) ^ value as u64;
	}

	fn write_reg_u32(&mut self, Reg(reg): Reg, value: u32) {
		self.registers.primary_registers[reg as usize] = value as u64;
	}

	fn write_reg_u64(&mut self, Reg(reg): Reg, value: u64) {
		self.registers.primary_registers[reg as usize] = value;
	}

	fn read_reg_u8(&mut self, Reg(reg): Reg) -> u8 {
		self.registers.primary_registers[reg as usize] as u8
	}

	fn read_reg_u16(&mut self, Reg(reg): Reg) -> u16 {
		self.registers.primary_registers[reg as usize] as u16
	}

	fn read_reg_u32(&mut self, Reg(reg): Reg) -> u32 {
		self.registers.primary_registers[reg as usize] as u32
	}

	fn read_reg_u64(&mut self, Reg(reg): Reg) -> u64 {
		self.registers.primary_registers[reg as usize]
	}

	read_write_rm!(u8);
	read_write_rm!(u16);
	read_write_rm!(u32);
	read_write_rm!(u64);

	/// Steps one instruction execution
	pub fn step_instruction(&mut self) {
		if let Err(interrupt) = try {
			let irq = IRQ.load(Ordering::Relaxed);
			IRQ.store(0, Ordering::Relaxed);
			if irq != 0 {
				Err(Interrupt::IRQ(irq))?;
			}
			let (instruction, size) = decode(&mut self.memory, self.instruction_pointer)?;
			match instruction {
				Instruction::In8 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					let value = self.devices.in_u8(operand0.0 as u16);
					self.write_reg_u8(A, value);
				}
				#[allow(unused)]
				Instruction::In16 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					fatal("16 bit devices are not implemented");
				}
				#[allow(unused)]
				Instruction::In32 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					fatal("32 bit devices are not implemented");
				}
				Instruction::In8D {} => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					let port = self.read_reg_u16(Reg(2));
					let value = self.devices.in_u8(port);
					self.write_reg_u8(A, value);
				}
				Instruction::In16D {} => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					fatal("16 bit devices are not implemented");
				}
				Instruction::In32D {} => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					fatal("32 bit devices are not implemented");
				}
				Instruction::IncRM8 { operand0 } => {
					let value = self.read_rm_u8(operand0)?.wrapping_add(1);
					self.write_rm_u8(operand0, value)?
				}
				Instruction::IncRM16 { operand0 } => {
					let value = self.read_rm_u16(operand0)?.wrapping_add(1);
					self.write_rm_u16(operand0, value)?
				}
				Instruction::IncRM32 { operand0 } => {
					let value = self.read_rm_u32(operand0)?.wrapping_add(1);
					self.write_rm_u32(operand0, value)?
				}
				Instruction::IncRM64 { operand0 } => {
					let value = self.read_rm_u64(operand0)?.wrapping_add(1);
					self.write_rm_u64(operand0, value)?
				}
				Instruction::Iret {} => {
					let rsp = self.read_reg_u64(SP);
					let instruction_pointer = self.memory.read_u64(rsp + 8)?;
					let rflags = self.memory.read_u64(rsp + 16)?;
					let stack_pointer = self.memory.read_u64(rsp + 24)?;
					self.instruction_pointer = instruction_pointer;
					self.rflags = rflags;
					self.write_reg_u64(SP, stack_pointer);
					self.cpl = ((rflags as i64) >> 32) as i8;
					return; // Skip incrementing the instruction pointer as
					// this changes the instruction pointer as part of
					// the instruction.
				}
				Instruction::JmpRel8 { operand0 } => {
					self.instruction_pointer = self
						.instruction_pointer
						.wrapping_add(operand0.0 as i8 as i64 as u64)
				}
				Instruction::JmpRel32 { operand0 } => {
					self.instruction_pointer = self
						.instruction_pointer
						.wrapping_add(operand0.0 as i32 as i64 as u64)
				}
				Instruction::MovReg8Imm { operand0, operand1 } => {
					self.write_reg_u8(operand0, operand1.0 as u8)
				}
				Instruction::MovReg16Imm { operand0, operand1 } => {
					self.write_reg_u16(operand0, operand1.0 as u16)
				}
				Instruction::MovReg32Imm { operand0, operand1 } => {
					self.write_reg_u32(operand0, operand1.0 as u32)
				}
				Instruction::MovReg64Imm { operand0, operand1 } => {
					self.write_reg_u64(operand0, operand1.0)
				}
				Instruction::MovReg8RM { operand0, operand1 } => {
					let value = self.read_rm_u8(operand1)?;
					self.write_reg_u8(operand0, value);
				}
				Instruction::MovReg16RM { operand0, operand1 } => {
					let value = self.read_rm_u16(operand1)?;
					self.write_reg_u16(operand0, value);
				}
				Instruction::MovReg32RM { operand0, operand1 } => {
					let value = self.read_rm_u32(operand1)?;
					self.write_reg_u32(operand0, value);
				}
				Instruction::MovReg64RM { operand0, operand1 } => {
					let value = self.read_rm_u64(operand1)?;
					self.write_reg_u64(operand0, value);
				}
				Instruction::MovRM8Reg { operand0, operand1 } => {
					let value = self.read_reg_u8(operand1);
					self.write_rm_u8(operand0, value)?;
				}
				Instruction::MovRM16Reg { operand0, operand1 } => {
					let value = self.read_reg_u16(operand1);
					self.write_rm_u16(operand0, value)?;
				}
				Instruction::MovRM32Reg { operand0, operand1 } => {
					let value = self.read_reg_u32(operand1);
					self.write_rm_u32(operand0, value)?;
				}
				Instruction::MovRM64Reg { operand0, operand1 } => {
					let value = self.read_reg_u64(operand1);
					self.write_rm_u64(operand0, value)?;
				}
				Instruction::Out8 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					let value = self.read_reg_u8(A);
					self.devices.out_u8(operand0.0 as u16, value);
				}
				#[allow(unused)]
				Instruction::Out16 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					fatal("16 bit devices are not implemented");
				}
				#[allow(unused)]
				Instruction::Out32 { operand0 } => {
					if self.cpl > 0 {
						Err(Interrupt::GeneralProtection)?;
					}
					let value = self.read_reg_u32(A);
					self.devices.out_u32(operand0.0 as u16, value);
				}
				Instruction::PopReg16 { operand0 } => {
					let rsp = self.read_reg_u64(SP);
					let value = self.memory.read_u16(rsp)?;
					self.write_reg_u64(SP, rsp.wrapping_add(2));
					self.write_reg_u16(operand0, value);
				}
				Instruction::PopReg64 { operand0 } => {
					let rsp = self.read_reg_u64(SP);
					let value = self.memory.read_u64(rsp)?;
					self.write_reg_u64(SP, rsp.wrapping_add(2));
					self.write_reg_u64(operand0, value);
				}
				Instruction::PushReg16 { operand0 } => {
					let value = self.read_reg_u16(operand0);
					let rsp = self.read_reg_u64(SP);
					self.write_reg_u64(SP, rsp.wrapping_sub(2));
					self.memory.write_u16(rsp.wrapping_sub(2), value)?;
				}
				Instruction::PushReg64 { operand0 } => {
					let value = self.read_reg_u64(operand0);
					let rsp = self.read_reg_u64(SP);
					self.write_reg_u64(SP, rsp.wrapping_sub(8));
					self.memory.write_u64(rsp.wrapping_sub(8), value)?;
				}
				Instruction::Swi4 { operand0 } => {
					let value = self.read_rm_u64(operand0)?;
					self.memory.swi4(value)
				}
				Instruction::Wrcr { operand0, operand1 } => {
					eprintln!(
						"Written 0x{:X} to config register 0x{:X}",
						self.read_rm_u64(operand1)?,
						operand0.0
					);
				}
			};
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

pub fn schedule_interrupt(irq: u8) {
	IRQ.store(irq, Ordering::Relaxed);
}
