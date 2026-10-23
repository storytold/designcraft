from pathlib import Path
import re,json,hashlib
src=Path('data/hu_HU.aff').read_bytes()
# Tailored to the pinned dictionary; fail on unexpected data, never decode lossily.
singles=set('COMPOUNDFLAG COMPOUNDFORBIDFLAG COMPOUNDPERMITFLAG NOSUGGEST KEEPCASE COMPOUNDFIRST COMPOUNDLAST ONLYROOT HU_KOTOHANGZO COMPOUNDBEGIN COMPOUNDEND NEEDAFFIX ONLYINCOMPOUND COMPOUNDROOT FORBIDDENWORD SUBSTANDARD GENERATE LEMMA_PRESENT'.split())
rows=[]; counts={}; af_left=0; rules_left=0
for n,line in enumerate(src.splitlines(),1):
    if not line.strip() or line.lstrip().startswith(b'#'):
        try: rows.append(line.decode('utf8'))
        except UnicodeDecodeError: rows.append(line.decode('iso8859-2'))
        continue
    parts=line.split(); key=parts[0].decode('ascii')
    if key=='FLAG': raise ValueError('Input unexpectedly specifies FLAG')
    if key=='AF':
        if af_left==0:
            af_left=int(parts[1]); rows.append(line.decode('ascii')); continue
        flags=parts[1]; converted=','.join(str(c) for c in flags)
        assert bytes(int(c) for c in converted.split(','))==flags
        rows.append('AF '+converted); af_left-=1
    elif key in singles or key in ('PFX','SFX'):
        assert len(parts[1])==1,(n,parts[:2])
        # Change only the flag token. Continuation classes are AF alias numbers.
        m=re.match(rb'(\S+\s+)(\S+)(.*)',line)
        rebuilt=m[1]+str(parts[1][0]).encode()+m[3]
        rows.append(rebuilt.decode('utf8'))
    elif key=='COMPOUNDRULE':
        if rules_left==0:
            rules_left=int(parts[1]); rows.append(line.decode('ascii')); continue
        pattern=parts[1]
        rows.append('COMPOUNDRULE '+''.join(chr(c) if c in b'*?' else '('+str(c)+')' for c in pattern)); rules_left-=1
    elif key=='NAME': rows.append(line.decode('iso8859-2'))
    else:
        rows.append(line.decode('utf8'))
        if key=='SET': rows.append('FLAG num')
    counts[key]=counts.get(key,0)+1
assert af_left==0 and rules_left==0
out=Path('data/converted'); out.mkdir(exist_ok=True)
(out/'hu_HU.aff').write_text('\n'.join(rows)+'\n', encoding='utf-8', newline='\n')
(out/'hu_HU.dic').write_bytes(Path('data/hu_HU.dic').read_bytes())
# Confirm aliases stayed unchanged in dictionary and continuation classes.
assert (out/'hu_HU.dic').read_bytes()==Path('data/hu_HU.dic').read_bytes()
for old,new in zip([l for l in src.splitlines() if l.startswith((b'PFX ',b'SFX '))],[l.encode() for l in rows if l.startswith(('PFX ','SFX '))]):
    assert old.split(maxsplit=2)[2]==new.split(maxsplit=2)[2]
Path('conversion.json').write_text(json.dumps({'directives':counts,'source_sha256':hashlib.sha256(src).hexdigest(),'converted_sha256':hashlib.sha256((out/'hu_HU.aff').read_bytes()).hexdigest(),'dictionary_unchanged':True,'flag_mode':'num; original byte values preserved'},indent=2), encoding='utf-8', newline='\n')
print('Converted flags without changing dictionary or affix payloads.')
