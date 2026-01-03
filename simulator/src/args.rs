use std::path::PathBuf;

#[derive(clap::Parser, Clone)]
pub struct Args {
	/// Path to config file
	pub config: PathBuf,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub enum MemoryType {
	RAM,
	ROM { path: PathBuf },
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Memory {
	pub start: u64,
	pub size: u64,
	pub memory_type: MemoryType,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub enum DeviceType {
	UTF8Console,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Device {
	pub port: u16,
	pub device_type: DeviceType,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Config {
	pub memory: Vec<Memory>,
	pub device: Vec<Device>,
}
