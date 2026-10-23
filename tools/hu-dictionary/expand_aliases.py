from pathlib import Path
import re,json
base=Path('data/converted');out=Path('data/expanded');out.mkdir(exist_ok=True)
lines=(base/'hu_HU.aff').read_text(encoding='utf-8').splitlines(); aliases=[]; expected=None
for line in lines:
 if line.startswith('AF '):
  if expected is None: expected=int(line.split()[1])
  else: aliases.append(line.split()[1])
assert len(aliases)==expected
def expand(value):
 assert value.isdigit(),value
 idx=int(value);assert 1<=idx<=len(aliases),value
 return aliases[idx-1]
rows=[]; continuation=0
for line in lines:
 if line.startswith('AF '):continue
 if line.startswith(('PFX ','SFX ')):
  parts=line.split()
  if len(parts)>=5 and '/' in parts[3]:
   word,alias=parts[3].rsplit('/',1)
   # Preserve text, conditions, morphology; only expand continuation flags.
   replacement=word+'/'+expand(alias)
   line=re.sub(r'^(\S+\s+\S+\s+\S+\s+)\S+',lambda m:m[1]+replacement,line)
   continuation+=1
 rows.append(line)
(out/'hu_HU.aff').write_text('\n'.join(rows)+'\n', encoding='utf-8', newline='\n')
dic=[]; stems=0
for line in (base/'hu_HU.dic').read_text(encoding='utf-8').splitlines():
 m=re.match(r'^(\S+)(.*)$',line)
 if m and '/' in m[1]:
  word,alias=m[1].rsplit('/',1)
  line=word+'/'+expand(alias)+m[2];stems+=1
 dic.append(line)
(out/'hu_HU.dic').write_text('\n'.join(dic)+'\n', encoding='utf-8', newline='\n')
# Verify each word and each resolved flag set corresponds to the alias input.
old=(base/'hu_HU.dic').read_text(encoding='utf-8').splitlines();new=(out/'hu_HU.dic').read_text(encoding='utf-8').splitlines()
assert len(old)==len(new)
for a,b in zip(old,new):
 if '/' not in a.split()[0]:assert a==b;continue
 ma=re.match(r'^(\S+)(.*)$',a);mb=re.match(r'^(\S+)(.*)$',b)
 wa,fa=ma[1].rsplit('/',1);wb,fb=mb[1].rsplit('/',1)
 assert wa==wb and ma[2]==mb[2] and expand(fa)==fb
Path('alias-expansion.json').write_text(json.dumps({'aliases':len(aliases),'expanded_dictionary_entries':stems,'expanded_continuations':continuation,'all_words_and_resolved_flags_verified':True},indent=2), encoding='utf-8', newline='\n')
print('Expanded',len(aliases),'aliases in',stems,'entries and',continuation,'continuations')
