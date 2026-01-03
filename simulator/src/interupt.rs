use std::fmt::Display;

#[derive(Debug)]
pub enum Interrupt {
	/// General Protection Interrupt. Unlike x86 this does not have any error code, since
	/// segments do not exist.
	GeneralProtection,

	/// Page fault interrupt. Identical to x86.
	PageFault {
		error_code: u32,
		cr2: u64,
	},

	/// Undefined exception. Identical to x86.
	Undefined,

	// Faault on fetch of interrupt. Identical to x86.
	DoubleFault,
}

impl Display for Interrupt {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Interrupt::GeneralProtection => write!(f, "GP"),
			Interrupt::PageFault { error_code, cr2 } => write!(f, "PF({error_code:X}, {cr2:X})"),
			Interrupt::Undefined => write!(f, "UD"),
			Interrupt::DoubleFault => write!(f, "DF"),
		}
	}
}

pub fn is_cannonical(address: u64) -> Result<(), Interrupt> {
	let shifted = address >> 47;
	if shifted == 0 || shifted == 0x1FFFF {
		Ok(())
	} else {
		Err(Interrupt::GeneralProtection)
	}
}

#[repr(C)]
pub struct InteruptDescriptorEntry {
	/// Marking this as a prsent entry
	pub present: bool,

	/// Disable interrupts on entry. Can be reenabled with sti or iretq. External irq mainly
	/// timer irq will wait and therefore the timers will be delayed.
	pub disable_interrupt: bool,

	/// Required privelage level. Only for software interrupts.
	pub rpl: i8,

	/// The location of the service_routine
	pub service_routine: u64,
}
