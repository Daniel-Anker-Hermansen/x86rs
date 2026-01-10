use std::{
	collections::HashMap,
	io::{Read, Write},
	sync::{
		Arc,
		atomic::{AtomicU8, Ordering},
	},
	thread,
	time::Duration,
};

use crate::state::schedule_interrupt;

pub trait Device {
	fn out_u8(&mut self, port: u16, byte: u8);

	fn in_u8(&mut self, port: u16) -> u8;
}

pub struct UTF8Console;

impl Device for UTF8Console {
	fn out_u8(&mut self, _port: u16, byte: u8) {
		let _ = std::io::stdout().write(&[byte]);
		let _ = std::io::stdout().flush();
	}

	fn in_u8(&mut self, _port: u16) -> u8 {
		let mut buf = [0];
		match std::io::stdin().read_exact(&mut buf) {
			Ok(_) => buf[0],
			Err(_) => 0xFF,
		}
	}
}

pub struct Timer {
	counter: u32,
	irq: u8,
	mode: Arc<AtomicU8>,
}

impl Timer {
	pub fn new(irq: u8) -> Timer {
		Timer {
			counter: 0,
			irq,
			mode: Arc::new(AtomicU8::new(0)),
		}
	}
}

fn run_timer(counter: u32, irq: u8, mode: Arc<AtomicU8>) {
	thread::spawn(move || {
		thread::sleep(Duration::from_micros(counter as u64));
		let timer_mode = mode.load(Ordering::Relaxed);
		if timer_mode & 0x01 == 0x01 {
			schedule_interrupt(irq);
			run_timer(counter, irq, mode);
		}
	});
}

impl Device for Timer {
	fn out_u8(&mut self, port: u16, byte: u8) {
		match port {
			0 => self.counter ^= (self.counter & 0xFF) ^ byte as u32,
			1 => self.counter ^= (self.counter & 0xFF00) ^ ((byte as u32) << 8),
			2 => self.counter ^= (self.counter & 0xFF0000) ^ ((byte as u32) << 16),
			3 => self.counter ^= (self.counter & 0xFF000000) ^ ((byte as u32) << 24),
			4 => {
				self.mode.store(byte, Ordering::Relaxed);
				run_timer(self.counter, self.irq, self.mode.clone());
			}
			_ => unreachable!(),
		}
	}

	fn in_u8(&mut self, _port: u16) -> u8 {
		0xFF
	}
}

pub struct PortDevices {
	devices: Vec<Box<dyn Device>>,
	ports: HashMap<u16, (usize, u16)>,
}
impl PortDevices {
	pub fn new() -> Self {
		Self {
			devices: Vec::new(),
			ports: HashMap::new(),
		}
	}

	pub fn add<T>(&mut self, ports: &[u16], device: T)
	where
		T: Device + 'static,
	{
		let index = self.devices.len();
		self.devices.push(Box::new(device));
		for (port, i) in ports.iter().zip(0..) {
			self.ports.insert(*port, (index, i));
		}
	}

	pub fn out_u8(&mut self, port: u16, byte: u8) {
		if let Some(&(device, port)) = self.ports.get(&port) {
			self.devices[device].out_u8(port, byte);
		}
	}

	pub fn out_u32(&mut self, port: u16, value: u32) {
		for (byte, port) in value.to_le_bytes().into_iter().zip(port..) {
			if let Some(&(device, port)) = self.ports.get(&port) {
				self.devices[device].out_u8(port, byte);
			}
		}
	}

	pub fn in_u8(&mut self, port: u16) -> u8 {
		match self.ports.get(&port) {
			Some(&(device, port)) => self.devices[device].in_u8(port),
			None => 0xFF,
		}
	}
}
