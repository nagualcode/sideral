import struct,sys
def isprime(n):
    if n<2: return False
    if n%2==0: return n==2
    i=3
    while i*i<=n:
        if n%i==0: return False
        i+=2
    return True
f=sys.argv[1]; d=open(f,'rb').read()[16:]
base=0x08020000
words=[(base+i,struct.unpack_from('<I',d,i)[0]) for i in range(0,len(d)-3,4)]
pr=[(hex(a),w) for a,w in words if 1000<=w<=3000000 and isprime(w)]
print("primes:",len(pr))
for a,w in pr: print(a,w, "%.4fs"%(w/48000))
# addresses referencing RAM regions
from collections import Counter
c=Counter((w>>24) for a,w in words if (w>>28) in (0x2,0x6,0xC,0xD,0x9))
print(c.most_common(10))
big=sorted(set(w for a,w in words if (w>>28) in (0xC,0xD,0x6,0x9) and (w&0xff)==0 ))
print([hex(x) for x in big[:60]])
