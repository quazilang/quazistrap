#!/usr/bin/env python3
"""Verify arithmetic operand requirements, reference loads and portable output."""
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

INVALID = {}
for operator, name in [("+", "add"), ("-", "sub"), ("*", "mul"), ("/", "div"), ("%", "mod")]:
    INVALID[f"owner_{name}"] = (f"struct Owner{{value:i32,}}fn main()void{{var a=Owner{{value:1}};var b=Owner{{value:2}};var c=a{operator}b;}}", "S06")
    INVALID[f"bool_{name}"] = (f"fn main()void{{var c=true{operator}false;}}", "S06")
    INVALID[f"generic_{name}"] = (f"fn calc[T](a:T,b:T)T{{ret a{operator}b;}}fn main()void{{calc[i32](5,2);calc[bool](true,false);}}", "S06")
    if operator != "+":
        INVALID[f"str_{name}"] = (f'fn main()void{{var c="a"{operator}"b";}}', "S06")
INVALID.update({
    "owner_neg": ("struct Owner{value:i32,}fn main()void{var a=Owner{value:1};var b=-a;}", "S06"),
    "array_neg": ("fn main()void{var a=[1,2];var b=-a;}", "S06"),
    "untyped_neg": ("enum E{One,}fn main()void{var a=E.One;var b=-a;}", "S14"),
    "untyped_add": ("enum E{One,}fn main()void{var a=E.One;var b=a+a;}", "S14"),
    "reference_neg": ("fn main()void{var a=1;var r=&a;var b=-r;}", "S06"),
    "generic_neg": ("fn neg[T](a:T)T{ret -a;}fn outer[U](a:U)U{ret neg[U](a);}fn main()void{outer[i32](1);outer[bool](true);}", "S06"),
    "owned_concat": ("struct String{data:*u8,}fn main()void{var a=String{data:0};var b=String{data:0};var c=a+b;}", "S06"),
    "generic_owned_concat": ("struct String{data:*u8,}fn add[T](a:T,b:T)T{ret a+b;}fn main()void{add[String](String{data:0},String{data:0});}", "S06"),
    "generic_ref": ("fn add[T](a:T,b:&T)T{ret a+b;}fn main()void{}", "S06"),
    "generic_pointer_mul": ("fn mul[T](a:T,b:i32)T{ret a*b;}unsafe fn main()void{mul[*u8](0,2);}", "S06"),
})
for name, expression in [("ptr_add", "p+p"), ("ptr_diff", "p-p"), ("ptr_rhs", "1+p"), ("ptr_mul", "p*2"), ("ptr_float", "p+1.5")]:
    INVALID[name] = (f"unsafe fn main()void{{var p:*u8=0;var result={expression};}}", "S06")

VALID = '''fn add[T](a:T,b:T)T{ret a+b;}
fn sub[T](a:T,b:T)T{ret a-b;}
fn mul[T](a:T,b:T)T{ret a*b;}
fn div[T](a:T,b:T)T{ret a/b;}
fn rem[T](a:T,b:T)T{ret a%b;}
fn neg[T](a:T)T{ret -a;}
fn outer[U](a:U,b:U)U{ret add[U](a,b);}
fn integers()bool{ret outer[i32](5,2)==7&&sub[i32](5,2)==3&&mul[i32](5,2)==10&&div[i32](5,2)==2&&rem[i32](5,2)==1&&neg[i32](5)==-5;}
fn frem(a:f64,b:f64)f64{var value=a;var reference=&value;ret *reference%b;}
fn floats()bool{ret add[f64](5.5,2.0)==7.5&&sub[f64](5.5,2.0)==3.5&&mul[f64](5.5,2.0)==11.0&&div[f64](5.5,2.0)==2.75&&frem(5.5,2.0)==1.5&&neg[f64](5.5)==-5.5&&frem(-5.5,2.0)==-1.5&&frem(5.5,-2.0)==1.5&&frem(__LARGE_FLOAT__,7.0)==1.0&&frem(1.0,__SMALLEST_FLOAT__)==0.0&&div[f64](1.0,frem(-0.0,2.0))<0.0&&frem(2.5,div[f64](1.0,0.0))==2.5&&frem(div[f64](1.0,0.0),2.0)!=frem(div[f64](1.0,0.0),2.0)&&frem(1.0,0.0)!=frem(1.0,0.0);}
type FloatRef=&f64;
fn aliases(a:FloatRef,b:FloatRef)f64{ret a+b;}
fn add_right[T](a:T,b:&f64)T{ret a+b;}
fn add_left[T](a:&f64,b:T)f64{ret a+b;}
fn references()bool{var a=1.5;var b=2.5;var ar=&a;var br=&b;ret aliases(ar,br)==4.0&&add_right[f64](1.5,br)==4.0&&add_left[f64](ar,2.5)==4.0;}
fn concat_left[T](a:&str,b:T)str{ret a+b;}
fn concat_right[T](a:T,b:&str!)T{ret a+b;}
fn nested_views()bool{var text="a";var shared=&text;var result=*shared+"b";ret result=="ab";}
fn mixed_views()bool{var text="a";var exclusive=&text!;var shared=&exclusive;var result=*shared+"b";ret result=="ab";}
fn views()bool{var a="a";var b="b";var ar=&a;var br=&b!;ret (ar+br)=="ab"&&concat_left[str](ar,"b")=="ab"&&concat_right[str]("a",br)=="ab"&&add[str]("a","b")=="ab"&&add[&str]("a","b")=="ab"&&nested_views()&&mixed_views();}
unsafe fn offset[T](a:T,b:i32)T{ret a+b-1;}
unsafe fn offset_reference[U](pointer:&*u8,amount:U)*u8{ret pointer+amount;}
fn pointers()bool{unsafe{var p:*u8=0;ret offset[*u8](p,4)==3&&offset_reference[i32](&p,4)==4;}}
struct Cell[T]{value:T,}
impl Cell[Item]{fn scaled(self:&Cell[Item],factor:Item)Item{ret self.value*factor;}}
fn impls()bool{var cell:Cell[f64]=Cell{value:1.5};ret cell.scaled(2.0)==3.0;}
fn magnitudes()bool{var value:u64=18446744073709551615;var signed:i64=-9223372036854775808;ret neg[u64](value)==1&&signed==-9223372036854775808;}
fn main()i32{if !integers(){ret 1;}if !floats(){ret 2;}if !references(){ret 3;}if !views(){ret 4;}if !pointers(){ret 5;}if !impls(){ret 6;}if !magnitudes(){ret 7;}ret 0;}
'''.replace('__LARGE_FLOAT__', format(1e300, '.1f')).replace('__SMALLEST_FLOAT__', format(math.ulp(0.0), '.324f'))


# Quazi float literals currently require decimal notation without exponents.
def check_remainder_bits(qz, root):
    work = root / "remainder_library"
    project(work, '@export("quazi_test_remainder")pub fn remainder(a:f64,b:f64)f64{ret a%b;}fn main()void{}')
    artifact = work / "remainder.qzi"
    common = ["--no-incremental", "--silent"]
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], work)
    library = work / "source.so"
    run([str(qz), "build", *common, "--shared-lib", "-o", str(library)], work)
    (work / "main.qz").unlink()
    emission = root / "remainder_artifact"
    emission.mkdir()
    portable = emission / "portable.so"
    obj = emission / "remainder.o"
    run([str(qz), "build", str(artifact), *common, "-c", "-o", str(obj)], emission)
    run(["ld", "-shared", "-o", str(portable), str(obj)], emission)
    cases = [(5.5, 2.0), (-5.5, 2.0), (5.5, -2.0), (-5.5, -2.0),
             (6.0, 2.0), (-6.0, 2.0), (0.0, 2.0), (-0.0, 2.0),
             (1.0e300, 7.0), (sys.float_info.max, math.ulp(0.0)),
             (math.ulp(0.0), 2.0 * math.ulp(0.0)),
             (1.0, 0.0), (1.0, -0.0), (math.inf, 2.0), (-math.inf, 2.0),
             (2.5, math.inf), (-0.0, -math.inf), (math.nan, 2.0), (2.0, math.nan)]
    rng = random.Random(26)
    for _ in range(1000):
        cases.append(tuple(struct.unpack("d", rng.getrandbits(64).to_bytes(8, sys.byteorder))[0] for _ in range(2)))
    for path in [library, portable]:
        module = ctypes.CDLL(str(path))
        remainder = module.quazi_test_remainder
        remainder.argtypes = [ctypes.c_double, ctypes.c_double]
        remainder.restype = ctypes.c_double
        for left, right in cases:
            try:
                expected = math.fmod(left, right)
            except ValueError:
                expected = math.nan
            actual = remainder(left, right)
            assert math.isnan(actual) if math.isnan(expected) else struct.pack("d", actual) == struct.pack("d", expected), (left, right, actual, expected)
    print(f"PASS floating remainder: {len(cases)} host bit comparisons each for native/source-removed QZI", flush=True)


def check_generic_zero_panics(qz, root):
    for name, expression in [("div", "ret a/b;"), ("mod", "ret a%b;"), ("div_assign", "a/=b;ret a;"), ("mod_assign", "a%=b;ret a;")]:
        work = root / f"panic_{name}"
        project(work, f"fn calc[T](a:T,b:T)T{{{expression}}}fn main()i32{{ret calc[i32](4,0);}}")
        manifest = work / "quazi.toml"
        manifest.write_text(manifest.read_text().replace("std=false\n", ""))
        binary = work / "native"
        common = ["--no-incremental", "--silent"]
        run([str(qz), "build", *common, "-o", str(binary)], work)
        assert "integer" in run([str(binary)], work, 101)
        artifact = work / "program.qzi"
        run([str(qz), "build", *common, "-i", "-o", str(artifact)], work)
        (work / "main.qz").unlink()
        emission = root / f"panic_artifact_{name}"
        emission.mkdir()
        binary = emission / "native"
        run([str(qz), "build", str(artifact), *common, "-o", str(binary)], emission)
        assert "integer" in run([str(binary)], emission, 101)
    print("PASS generic division/remainder zero: 4 native/source-removed QZI panics with exit101", flush=True)


def check(qz, root, before):
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    for name, (source, code) in INVALID.items():
        work = root / name
        project(work, source)
        for options in [["check"], ["build", *common], ["build", *common, "-i"], ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and code in result.stdout, (name, options, result.stdout)
    print(f"PASS arithmetic: {len(INVALID)} rejected check/native/QZI/Windows COFF", flush=True)
    work = root / "source"
    project(work, VALID)
    run([str(qz), "check"], work)
    binary = work / "native"
    run([str(qz), "build", *common, "-o", str(binary)], work)
    run([str(binary)], work)
    artifact = work / "program.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], work)
    (work / "main.qz").unlink()
    emission = root / "artifact"
    emission.mkdir()
    binary = emission / "native"
    run([str(qz), "build", str(artifact), *common, "-o", str(binary)], emission)
    run([str(binary)], emission)
    coff = emission / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS arithmetic: 7 valid checks native/source-removed QZI/Windows COFF", flush=True)
    check_generic_zero_panics(qz, root)
    check_remainder_bits(qz, root)

    if before:
        invalid = root / "invalid_cache"
        project(invalid, INVALID["owner_neg"][0])
        options = ["build", "--no-color", "--no-unicode", "-o", str(invalid / "native")]
        # Compile only: never execute the old invalid owning-handle arithmetic.
        run([str(before), *options], invalid)
        result = subprocess.run([str(qz), *options], cwd=invalid, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode != 0 and "S06" in result.stdout, result.stdout
        safe = root / "valid_cache"
        project(safe, 'fn rem(a:f64,b:f64)f64{ret a%b;}fn main()i32{if rem(5.5,2.0)==1.5{ret 0;}ret 1;}')
        binary = safe / "native"
        options = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
        run([str(before), *options], safe)
        # Nonzero finite bit patterns give a defined old integer division with
        # a wrong numeric result. No old reference/owner/string code executes.
        run([str(binary)], safe, 1)
        for expected in ["miss", "hit"]:
            output = run([str(qz), *options], safe)
            assert f"cache lookup {expected}" in " ".join(output.split()), output
            run([str(binary)], safe)
        print("PASS arithmetic caches: old invalid source rejected, float remainder rebuild/new hit correct")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-arithmetic-") as directory:
        check(args.qz.resolve(), Path(directory), args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
