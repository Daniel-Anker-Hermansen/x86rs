# x86 Ridiculously Simplified (x86rs)

This is a simulatur for x86 Ridiculously Simplified. The aim of this architecture is to behave according to the x86 specification for ring 3 in long mode, while rebuilding everything else from scrath to only support the features of ring 0 and -1 which are actually used by real operating systems.

# Modes

The CPU has three modes: Hypervisor (-1), Supervisor (0), User (3). The Hypervisor will only be a available with the virtualization feature, which will likely not be implemented (for a long time at least). All modes run with 64 bit addressing, and a flat memory model, with 48 bits (or 57 bits when the five level paging feature is implemented) of addressable virtual memory.

# Boot

On boot the cr3 register will have the linear address 0, and four level paging will be used. Therefore a user should connect the first page to a hardware mapping such that this contains a valid page table. rip will be set to 0. The paging tables should therefore map this to a physical address which contains boot code.

# Interrupts

Interrupts and faults are handled by the service routines in the idt. The stack used is the special interupt stack. One can load a stack pointer with `list`, when in ring 3. When in ring 0, the current stack is used. All stack can therefore be overwritten by an interrupt when in ring 0.
