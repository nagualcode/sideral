import re,struct,sys,math
binf,src,dst=sys.argv[1:4]
d=open(binf,'rb').read(); base=0x08020000
def val(a):
    o=a-base
    if o<0 or o+4>len(d): return None
    u=struct.unpack_from('<I',d,o)[0]; f=struct.unpack_from('<f',d,o)[0]
    if (u>>24) in (0x20,0x40,0xc0,0xd0,0x08,0xe0,0xa0): return "@%08x"%u
    if u!=0 and math.isfinite(f) and 1e-12<abs(f)<1e10 and (u & 0x7f800000)!=0: return "%.9g"%f
    return "0x%x(%d)"%(u,u)
def rep(m):
    a=int(m.group(1),16); v=val(a)
    return "K[%s]"%v if v else m.group(0)
t=open(src).read()
t=re.sub(r'DAT_(0802[0-9a-f]{4}|0803[0-9a-f]{4})',rep,t)
open(dst,'w').write(t)
