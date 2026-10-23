#!/usr/bin/env python3
"""Verify ELF producer stack metadata through source, QZI and native linking."""
import argparse
import ctypes
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
from check_numeric_mutations import project, run

COMMON = ['--no-incremental', '--silent', '--no-color', '--no-unicode']
SOURCE = '@export("quazi_stack_answer")pub fn answer()i32{ret 42;}fn main()i32{ret 0;}'


def header(data):
    assert data[:6] == b'\x7fELF\x02\x01', 'expected little endian ELF64'
    return struct.unpack_from('<16sHHIQQQIHHHHHH', data)


def marker(data, expected=True):
    elf = header(data)
    table, stride, count, names_index = elf[6], elf[11], elf[12], elf[13]
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, table + stride * i) for i in range(count)]
    names_section = sections[names_index]
    names = data[names_section[4]:names_section[4] + names_section[5]]
    matches = [s for s in sections if names[s[0]:].split(b'\0', 1)[0] == b'.note.GNU-stack']
    if not expected:
        assert not matches, 'baseline unexpectedly already has GNU-stack metadata'
        return
    assert len(matches) == 1, 'ELF object lacks a unique GNU-stack marker'
    section = matches[0]
    assert section[1] == 1 and section[2] == 0 and section[5] == 0, section  # PROGBITS, no flags, empty


def stack(path, executable=False):
    data = path.read_bytes()
    elf = header(data)
    table, stride, count = elf[5], elf[9], elf[10]
    headers = [struct.unpack_from('<IIQQQQQQ', data, table + stride * i) for i in range(count)]
    stacks = [p for p in headers if p[0] == 0x6474e551]
    assert len(stacks) == 1, (path, 'missing or duplicate PT_GNU_STACK')
    assert bool(stacks[0][1] & 1) == executable, (path, stacks[0])
    assert stacks[0][1] & 6 == 6, (path, 'stack must be readable/writable')


def check(qz, root, before, expect_old_hit=False):
    work = root / 'source'
    project(work, SOURCE)
    if before:
        obj = work / 'old.o'
        run([str(before), 'build', *COMMON, '-c', '-o', str(obj)], work)
        marker(obj.read_bytes(), expected=False)
        # The metadata is emitted anew even when QZI/QZC bodies remain valid.
        cached = work / 'cached.so'
        run([str(before), 'build', '--shared-lib', '--no-color', '--no-unicode', '-o', str(cached)], work)
        output = run([str(qz), 'build', '--shared-lib', '--no-color', '--no-unicode', '-o', str(cached)], work)
        if expect_old_hit:
            assert 'cache lookup hit' in ' '.join(output.split()), output
        # Debug/release compiler identities intentionally have separate caches.
        output = run([str(qz), 'build', '--shared-lib', '--no-color', '--no-unicode', '-o', str(cached)], work)
        assert 'cache lookup hit' in ' '.join(output.split()), output
        stack(cached)
        assert ctypes.CDLL(str(cached)).quazi_stack_answer() == 42
        print('PASS baseline: previous compiler object lacks stack marker', flush=True)
    for mode in ['builtin', 'ld']:
        binary = work / f'executable_{mode}'
        run([str(qz), 'build', *COMMON, '--linker', mode, '-o', str(binary)], work)
        stack(binary)
        run([str(binary)], work)
    obj = work / 'source.o'
    run([str(qz), 'build', *COMMON, '-c', '-o', str(obj)], work)
    marker(obj.read_bytes())
    archive = work / 'source.a'
    run([str(qz), 'build', *COMMON, '--static-lib', '-o', str(archive)], work)
    members = run(['ar', 't', str(archive)], work).splitlines()
    assert members
    for member in members:
        marker(subprocess.check_output(['ar', 'p', str(archive), member], cwd=work))
    artifact = work / 'program.qzi'
    run([str(qz), 'build', *COMMON, '-i', '-o', str(artifact)], work)
    library = work / 'source.so'
    run([str(qz), 'build', *COMMON, '--shared-lib', '-o', str(library)], work)
    stack(library)
    assert ctypes.CDLL(str(library)).quazi_stack_answer() == 42
    # Inspect user-requested executable-stack output without loading it.
    manifest = work / 'quazi.toml'
    manifest.write_text(manifest.read_text() + '\n[link]\nflags=["-z","execstack"]\n')
    requested = work / 'requested.so'
    run([str(qz), 'build', *COMMON, '--shared-lib', '-o', str(requested)], work)
    stack(requested, executable=True)
    (work / 'main.qz').unlink()
    emission = root / 'artifact'
    emission.mkdir()
    obj = emission / 'portable.o'
    run([str(qz), 'build', str(artifact), *COMMON, '-c', '-o', str(obj)], emission)
    marker(obj.read_bytes())
    for mode in ['builtin', 'ld']:
        binary = emission / f'executable_{mode}'
        run([str(qz), 'build', str(artifact), *COMMON, '--linker', mode, '-o', str(binary)], emission)
        stack(binary)
        run([str(binary)], emission)
    library = emission / 'portable.so'
    run([str(qz), 'build', str(artifact), *COMMON, '--shared-lib', '-o', str(library)], emission)
    stack(library)
    assert ctypes.CDLL(str(library)).quazi_stack_answer() == 42
    coff = emission / 'portable.obj'
    run([str(qz), 'build', str(artifact), *COMMON, '-c', '--target', 'x86_64-windows', '-o', str(coff)], emission)
    assert coff.read_bytes()[:2] == b'\x64\x86'
    assert b'.note.GNU-stack' not in coff.read_bytes()
    print('PASS ELF stack: source/QZI objects, static archive, builtin/ld executables, default shared loading, execstack override, COFF control', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qz', type=Path, default=Path('target/debug/qz'))
    parser.add_argument('--before-qz', type=Path)
    parser.add_argument('--expect-old-cache-hit', action='store_true', help='require matching baseline/current compiler profiles and an old exact hit')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='quazi-stack-') as temporary:
        assert not args.expect_old_cache_hit or args.before_qz, '--expect-old-cache-hit requires --before-qz'
        check(args.qz.resolve(), Path(temporary), args.before_qz.resolve() if args.before_qz else None, args.expect_old_cache_hit)


if __name__ == '__main__':
    main()
