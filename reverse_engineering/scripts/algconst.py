import re,sys
t=open(sys.argv[1]).read()
isr=re.search(r'===== vec_44.*?(?======)',t,re.S).group(0)
disp=re.findall(r'== (0x[0-9a-f]+|\d)\) \{\s*(FUN_[0-9a-f]+)\(\);',isr)
def isp(n):
    if n<2: return False
    i=2
    while i*i<=n:
        if n%i==0: return False
        i+=1
    return True
for k,f in disp:
    m=re.search(r'===== %s.*?(?======)'%f,t,re.S); body=m.group(0)
    wraps=sorted(set(int(x) for x in re.findall(r'K\[0x[0-9a-f]+\((\d+)\)\] <=? iVar',body)) | set(int(x,16) for x in re.findall(r'if \((0x[0-9a-f]+) < iVar',body)))
    offs=sorted(set(int(x) for x in re.findall(r'K\[0x[0-9a-f]+\((\d+)\)\] \+ iVar',body)))
    print(k,f,'wraps',[(w,w+1 if isp(w+1) else '') for w in wraps if w>40],'offs',offs)
