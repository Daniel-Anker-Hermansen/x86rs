use std::{
	collections::HashMap,
	io::{Read, Write},
};

pub trait Device {
	fn out_u8(&mut self, byte: u8);

	fn in_u8(&mut self) -> u8;
}

pub struct UTF8Console;

impl Device for UTF8Console {
	fn out_u8(&mut self, byte: u8) {
		let _ = std::io::stdout().write(&[byte]);
		let _ = std::io::stdout().flush();
	}

	fn in_u8(&mut self) -> u8 {
		let mut buf = [0];
		match std::io::stdin().read_exact(&mut buf) {
			Ok(_) => buf[0],
			Err(_) => 0xFF,
		}
	}
}

pub struct PortDevices {
	devices: HashMap<u16, Box<dyn Device>>,
}
impl PortDevices {
	pub fn new() -> Self {
		Self {
			devices: HashMap::new(),
		}
	}

	pub fn add<T>(&mut self, port: u16, device: T)
	where
		T: Device + 'static,
	{
		self.devices.insert(port, Box::new(device));
	}

	pub fn out_u8(&mut self, port: u16, byte: u8) {
		if let Some(device) = self.devices.get_mut(&port) {
			device.out_u8(byte);
		}
	}

	pub fn in_u8(&mut self, port: u16) -> u8 {
		match self.devices.get_mut(&port) {
			Some(device) => device.in_u8(),
			None => 0xFF,
		}
	}
}
