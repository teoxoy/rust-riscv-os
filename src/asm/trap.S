.global asm_trap_vector
# This will be our trap vector when we start
# handling interrupts.
asm_trap_vector:
	mret