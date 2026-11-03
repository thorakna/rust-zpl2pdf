import sys, json; sys.path.insert(0, sys.argv[1])
from labelary import render
from bbox import comps
S=100; K=10
chars=[chr(c) for c in range(32,127)]+[chr(c) for c in range(0xA1,0x100) if c!=0xAD]+list('ĞğİıŞşŒœŠšŽžŸ€‘’“”•–—…™')
def fd(s):
    if any(ch in s for ch in '^~_'):
        return '^FH^FD'+''.join('_%02X'%ord(ch) if ch in '^~_' else ch for ch in s)
    return '^FD'+s
p=render(f'^XA^CI28^FO20,20^A0N,{S},{S}^FD'+'H'*20+'^FS^XZ', size='15x3')
c,r=comps(p,0,200); starts=[a for a,b in c]
advH=(starts[-1]-starts[0])/19; inkH=c[0][1]-c[0][0]+1
print('advH',advH/S,'inkH',inkH, file=sys.stderr)
res={}
lineh=125; rows=24
for i in range(0,len(chars),rows):
    chunk=chars[i:i+rows]; lines=[]; meta=[]
    for j,ch in enumerate(chunk):
        x=20; y=20+j*lineh
        lines.append(f'^FO{x},{y}^A0N,{S},{S}'+fd('H'+ch*K+'H')+'^FS'); meta.append((ch,x,y))
    p=render('^XA^CI28'+''.join(lines)+'^XZ', size='8x15')
    for ch,x,y in meta:
        c,r=comps(p,y,y+76)
        first=c[0][0]; lastL=c[-1][1]-inkH+1
        res[ch]=round(((lastL-first)-advH)/(S*K),4)
res['H']=round(advH/S,4)
json.dump(res, open(sys.argv[2],'w',encoding='utf-8'), ensure_ascii=False, indent=0)
