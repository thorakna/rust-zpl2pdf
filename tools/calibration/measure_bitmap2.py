import sys; sys.path.insert(0, sys.argv[1])
from labelary import render
from bbox import comps
fonts = {'A':(9,5),'B':(11,7),'C':(18,10),'D':(18,10),'E':(28,15),'F':(26,13),'G':(60,40),'H':(21,13)}
for mag in (1,3):
    lines=[]; y=20; meta=[]
    for f,(h,w) in fonts.items():
        for s in ('HHH','ggg','xxx','ÉÉÉ'):
            lines.append(f'^FO20,{y}^A{f}N,{h*mag},{w*mag}^FD{s}^FS'); meta.append((f,s,y,h*mag)); y += h*mag+12*mag
    zpl='^XA^CI28'+''.join(lines)+'^XZ'
    p=render(zpl, size='4x15')
    print('mag',mag)
    for f,s,y,h in meta:
        c,r=comps(p, y-6*mag, y+h+6*mag)
        print(' ',f,s,'rows',[(a-y,b-y) for a,b in r],'cols',c[:3])
