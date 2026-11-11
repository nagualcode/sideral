import sys,capstone
d=open(sys.argv[1],'rb').read(); base=0x08020000
a=int(sys.argv[2],16); n=int(sys.argv[3],0)
md=capstone.Cs(capstone.CS_ARCH_ARM, capstone.CS_MODE_THUMB|capstone.CS_MODE_MCLASS); md.detail=False
for i in md.disasm(d[a-base:a-base+n], a): print(hex(i.address), i.mnemonic, i.op_str)
