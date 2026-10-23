#!/usr/bin/env python3
"""Verify D-016 exact integer and typed floating power through source and QZI."""
import argparse
import ctypes
import math
import random
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
from check_numeric_mutations import project, run
from check_operator_requirements import artifacts

COMMON = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
INVALID = [
    'fn main()void{2**-2;}',
    'fn main()void{const n=-2;2**n;}',
    'fn main()void{(2 as i8)**7;}',
    'fn main()void{(2 as u8)**8;}',
    'fn main()void{(2 as i64)**63;}',
    'fn main()void{(2 as u64)**64;}',
    'fn main()void{true**false;}',
    'fn main()void{"a"**2;}',
    'struct Owner{value:i32,}fn main()void{Owner{value:2}**2;}',
    'fn power[T](a:T,b:T)T{ret a**b;}fn main()void{power[bool](true,false);}',
    'fn power[T,U](a:T,b:U)T{ret a**b;}fn main()void{power[i32,f64](2,0.5);}',
    'fn power[T](a:T)T{ret a**-2;}fn main()void{power[i32](2);}',
]

VALUES = '''fn power[T](a:T,b:T)T{ret a**b;}
fn wrap[U](a:U,b:U)U{ret power[U](a,b);}
fn small()bool{ret power[i8](-2,7)==-128&&power[u8](2,7)==128&&power[i16](-2,15)==-32768&&power[u16](2,15)==32768;}
fn large()bool{ret wrap[i64](3,33)==5559060566555523&&power[u64](3,40)==12157665459056928801&&power[u64](18446744073709551615,1)==18446744073709551615;}
fn boundaries()bool{ret power[i64](-9223372036854775808,1)==-9223372036854775808&&power[i64](-2,63)==-9223372036854775808&&power[i64](0,0)==1&&power[u64](1,18446744073709551615)==1&&power[u64](0,18446744073709551615)==0;}
fn widths()bool{ret power[i32](-2,31)==-2147483648&&power[u32](2,31)==2147483648&&power[isize](-2,63)==-9223372036854775808&&power[usize](2,63)==9223372036854775808;}
fn mixed(a:i32,b:f64)f64{ret a**b;}
fn floating()bool{ret power[f64](2.0,-2.0)==0.25&&mixed(4,0.5)==2.0;}
type Scalar=i64;
fn borrowed(a:&Scalar,b:&i32)Scalar{ret a**b;}
fn references()bool{var a:Scalar=3;var b=5;ret borrowed(&a,&b)==243;}
fn sequence()bool{var a=2;var value=a**a++;if value!=4||a!=3{ret false;}a=2;var second=a++**a++;ret second==8&&a==4;}
struct Cell[T]{value:T,}
impl Cell[Item]{fn raised(self:&Cell[Item],exponent:Item)Item{ret self.value**exponent;}}
fn methods()bool{var cell:Cell[i64]=Cell{value:3 as i64};ret cell.raised(5)==243;}
fn main()i32{if !small(){ret 1;}if !large(){ret 2;}if !boundaries(){ret 3;}if !floating(){ret 4;}if !references(){ret 5;}if !sequence(){ret 6;}if !methods(){ret 7;}if !widths(){ret 8;}ret 0;}
'''


def check_panics(qz, root):
    cases = [
        ('mutable_negative', 'fn main()void{var n=-2;2**n;}', 'nonnegative'),
        ('parameter_negative', 'fn power[T](a:T,b:T)T{ret a**b;}fn main()void{power[i32](2,-2);}', 'nonnegative'),
        ('signed_overflow', 'fn power(a:i8,b:i8)i8{ret a**b;}fn main()void{power(2,7);}', 'overflow'),
        ('unsigned_overflow', 'fn power(a:u64,b:u64)u64{ret a**b;}fn main()void{power(2,64);}', 'overflow'),
        ('minimum_squared', 'fn power(a:i64,b:i64)i64{ret a**b;}fn main()void{power(-9223372036854775808,2);}', 'overflow'),
    ]
    for ty, exponent in [('i16',15),('i32',31),('isize',63),('u8',8),('u16',16),('u32',32),('usize',64)]:
        cases.append((f'{ty}_overflow', f'fn power(a:{ty},b:{ty}){ty}{{ret a**b;}}fn main()void{{power(2,{exponent});}}', 'overflow'))
    cases.append(('u64_max_squared', 'fn power(a:u64,b:u64)u64{ret a**b;}fn main()void{power(18446744073709551615,2);}', 'overflow'))
    for name, source, message in cases:
        work = root / name
        project(work, source)
        manifest = work / 'quazi.toml'
        manifest.write_text(manifest.read_text().replace('std=false\n', ''))
        binary = work / 'native'
        run([str(qz), 'build', *COMMON, '-o', str(binary)], work)
        assert message in run([str(binary)], work, 101)
        artifact = work / 'program.qzi'
        run([str(qz), 'build', *COMMON, '-i', '-o', str(artifact)], work)
        (work / 'main.qz').unlink()
        emission = root / (name + '_artifact')
        emission.mkdir()
        binary = emission / 'native'
        run([str(qz), 'build', str(artifact), *COMMON, '-o', str(binary)], emission)
        assert message in run([str(binary)], emission, 101)
    print(f'PASS power: {len(cases)} native/source-removed QZI language panics', flush=True)


def check_exports(qz, root):
    work = root / 'exports'
    project(work, '''@export("quazi_test_integer_power")pub fn integer_power(a:u64,b:u64)u64{ret a**b;}
@export("quazi_test_signed_power")pub fn signed_power(a:i64,b:i64)i64{ret a**b;}
@export("quazi_test_float_power")pub fn float_power(a:f64,b:f64)f64{ret a**b;}
@export("quazi_test_float_integer_power")pub fn float_integer_power(a:f64,b:i64)f64{ret a**b;}
fn main()void{}''')
    artifact = work / 'exports.qzi'
    run([str(qz), 'build', *COMMON, '-i', '-o', str(artifact)], work)
    source_lib = work / 'source.so'
    run([str(qz), 'build', *COMMON, '--shared-lib', '-o', str(source_lib)], work)
    (work / 'main.qz').unlink()
    emission = root / 'export_artifact'
    emission.mkdir()
    obj = emission / 'portable.o'
    run([str(qz), 'build', str(artifact), *COMMON, '-c', '-o', str(obj)], emission)
    portable_lib = work / 'portable.so'
    run(['ld', '-shared', '-o', str(portable_lib), str(obj)], work)
    assert ' pow' not in run(['nm', '-u', str(obj)], work)
    rng = random.Random(16)
    integers = [(0, 0), (2**64-1, 1), (3, 40), (1, 2**64-1)]
    integers += [(rng.randrange(0, 1000), rng.randrange(0, 7)) for _ in range(200)]
    floats = [(2.0, -2.0), (4.0, 0.5), (-2.0, 3.0), (-2.0, 0.5),
              (math.nan, 0.0), (1.0, math.nan), (-0.0, -3.0),
              (math.inf, -2.0), (-math.inf, 3.0)]
    floats += [(2**rng.uniform(-100, 100), rng.uniform(-10, 10)) for _ in range(200)]
    for path in [source_lib, portable_lib]:
        module = ctypes.CDLL(str(path))
        integer = module.quazi_test_integer_power
        integer.argtypes = [ctypes.c_uint64, ctypes.c_uint64]
        integer.restype = ctypes.c_uint64
        for base, exponent in integers:
            assert integer(base, exponent) == base**exponent, (base, exponent)
        signed = module.quazi_test_signed_power
        signed.argtypes = [ctypes.c_int64, ctypes.c_int64]
        signed.restype = ctypes.c_int64
        for base, exponent in [(-2,63),(-9223372036854775808,1),(-3,33)]+[(a if a%2 else -a,b) for a,b in integers[4:]]:
            assert signed(base,exponent) == base**exponent, (base,exponent)
        floating = module.quazi_test_float_power
        floating.argtypes = [ctypes.c_double, ctypes.c_double]
        floating.restype = ctypes.c_double
        for base, exponent in floats:
            if base == 0.0 and exponent < 0:
                expected = -math.inf if math.copysign(1, base) < 0 and exponent % 2 else math.inf
            else:
                try:
                    expected = math.pow(base, exponent)
                except ValueError:
                    expected = math.nan
            actual = floating(base, exponent)
            if math.isnan(expected):
                assert math.isnan(actual), (base, exponent, actual)
            else:
                assert actual == expected or math.isclose(actual, expected, rel_tol=2e-13), (base, exponent, actual, expected)
        fi = module.quazi_test_float_integer_power
        fi.argtypes = [ctypes.c_double, ctypes.c_int64]
        fi.restype = ctypes.c_double
        assert fi(2.0, -1074) == math.ulp(0.0)
        assert fi(-1.0, 9007199254740993) == -1.0
        assert fi(-1.0, -9223372036854775808) == 1.0
    print(f'PASS power exports: {len(integers)} exact integer and {len(floats)} floating cases per source/QZI library', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qz', type=Path, default=Path('target/debug/qz'))
    parser.add_argument('--before-qz', type=Path)
    args = parser.parse_args()
    qz = args.qz.resolve()
    with tempfile.TemporaryDirectory(prefix='quazi-power-') as directory:
        root = Path(directory)
        for index, source in enumerate(INVALID):
            work = root / f'invalid_{index}'
            project(work, source)
            for options in [['check'], ['build', *COMMON], ['build', *COMMON, '-i'], ['build', *COMMON, '-c', '--target', 'x86_64-windows']]:
                result = subprocess.run([str(qz), *options], cwd=work, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
                assert result.returncode != 0 and ('S14' if index == 10 else 'S06') in result.stdout, (index, options, result.stdout)
        print(f'PASS power: {len(INVALID)} invalid programs rejected check/native/QZI/COFF', flush=True)
        artifacts(qz, root, 'values', VALUES)
        print('PASS power: 8 value groups native/source-removed QZI/Windows COFF', flush=True)
        check_panics(qz, root)
        check_exports(qz, root)
        if args.before_qz:
            before = args.before_qz.resolve()
            invalid = root / 'old_invalid'
            project(invalid, INVALID[8])
            options = ['build', '-i', '--no-color', '--no-unicode']
            # Compile bytecode only: never execute old handle arithmetic.
            run([str(before), *options], invalid)
            result = subprocess.run([str(qz), *options], cwd=invalid, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and 'S06' in result.stdout, result.stdout
            valid = root / 'old_valid'
            project(valid, 'fn power(a:u64,b:u64)u64{ret a**b;}fn main()i32{if power(3,40)==12157665459056928801{ret 0;}ret 1;}')
            run([str(before), *options], valid)
            binary = valid / 'native'
            for expected in ['miss', 'hit']:
                output = run([str(qz), 'build', '--no-color', '--no-unicode', '-o', str(binary)], valid)
                assert f'cache lookup {expected}' in ' '.join(output.split()), output
                run([str(binary)], valid)
            print('PASS power caches: old nonnumeric source rejected; exact integer rebuild/miss/new hit', flush=True)


if __name__ == '__main__':
    main()
