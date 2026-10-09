"""Reports what kind of file an export produced, for the smoke test."""
import sys, zipfile

path = sys.argv[1]
head = open(path, 'rb').read(32)
if head.startswith(b'%PDF'):
    print('PDF header ok')
elif head.startswith(b'PK'):
    z = zipfile.ZipFile(path)
    ok = 'ok' if z.testzip() is None else 'BAD'
    print(f'zip ok, {len(z.namelist())} entries, integrity {ok}')
elif head.lstrip().lower().startswith(b'<!doctype html'):
    print('html page')
else:
    print('plain text')
