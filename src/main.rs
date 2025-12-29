#![feature(array_try_from_fn)]
#![feature(btree_cursors)]
#![feature(try_blocks)]

use clap::Parser;

use args::{Args, Config};
use memory::{
	ConventionalMemory, MemoryManagementUnit, PhysicalMemoryManagementUnit, ReadOnlyMemory,
};
use state::ProcessorState;

use crate::device::{PortDevices, UTF8Console};

mod args;
mod decode;
mod device;
mod error;
mod instruction;
mod interupt;
mod memory;
mod state;

fn main() {
	let args = Args::parse();
	let config = std::fs::read_to_string(args.config).unwrap();
	let toml: Config = toml::from_str(&config).unwrap();

	let mut memory_management_unit = PhysicalMemoryManagementUnit::new();
	for memory in &toml.memory {
		match &memory.memory_type {
			args::MemoryType::RAM => memory_management_unit.add(memory.start, memory.size, || {
				ConventionalMemory::create(memory.size)
			}),
			args::MemoryType::ROM { path } => {
				let data = std::fs::read(path).unwrap();
				memory_management_unit.add(memory.start, memory.size, || {
					ReadOnlyMemory::create(&data, memory.size)
				})
			}
		}
	}

	let mut devices = PortDevices::new();

	for device in &toml.device {
		match device.device_type {
			args::DeviceType::UTF8Console => devices.add(device.port, UTF8Console),
		}
	}

	let memory = MemoryManagementUnit::new(memory_management_unit);
	let mut state = ProcessorState::new(memory, devices);

	loop {
		state.step_instruction();
		//state.eprint_primary_registers();
	}
}
