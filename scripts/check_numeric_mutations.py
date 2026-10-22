#!/usr/bin/env python3
"""Check source numeric mutation obligations, artifacts and old-cache rejection."""

import argparse
import subprocess
import tempfile
from pathlib import Path

INVALID = {
    "struct_increment": ("struct Owner{value:i32,}fn main()void{var owner=Owner{value:1};owner++;}", "S06"),
    "enum_increment": ("enum E{One,}fn main()void{var e:E=E.One;e++;}", "S06"),
    "untyped_enum_increment": ("enum E{One,}fn main()void{var e=E.One;e++;}", "S14"),
    "untyped_enum_compound": ("enum E{One,}fn main()void{var e=E.One;e+=E.One;}", "S14"),
    "array_increment": ("fn main()void{var a=[1,2];a++;}", "S06"),
    "bool_increment": ("fn main()void{var b=true;--b;}", "S06"),
    "str_increment": ('fn main()void{var s="text";s++;}', "S06"),
    "fn_increment": ("fn one()i32{ret 1;}fn main()void{var f:fn()i32=one;f++;}", "S06"),
    "struct_compound": ("struct Owner{value:i32,}fn main()void{var a=Owner{value:1};var b=Owner{value:2};a+=b;}", "S06"),
    "str_compound": ('fn main()void{var s="a";s+="b";}', "S06"),
    "array_compound": ("fn main()void{var a=[1,2];a*=[3,4];}", "S06"),
    "bool_compound": ("fn main()void{var b=true;b+=false;}", "S06"),
    "bad_rhs": ("fn main()void{var n=1;n+=true;}", "S06"),
    "pointer_increment": ("unsafe fn main()void{var p:*u8=0;p++;}", "S06"),
    "pointer_multiply": ("unsafe fn main()void{var p:*u8=0;p*=2;}", "S06"),
    "pointer_float_offset": ("unsafe fn main()void{var p:*u8=0;p+=1.5;}", "S06"),
    "generic_increment": ("fn bump[T](v:T)T{v++;ret v;}fn main()void{bump[i32](1);bump[bool](true);}", "S06"),
    "generic_compound": ('fn add[T](v:T,r:T)T{v+=r;ret v;}fn main()void{add[i32](1,2);add[str]("a","b");}', "S06"),
    "transitive_generic": ("fn bump[T](v:T)T{v++;ret v;}fn outer[U](v:U)U{ret bump[U](v);}fn main()void{outer[bool](true);}", "S06"),
    "generic_rhs_slot": ("fn add[Marker,U](v:i32,r:U)i32{v+=r;ret v;}fn main()void{add[i32,bool](1,true);}", "S06"),
    "generic_impl": ("struct Cell[T]{value:T,}impl Cell[Item]{fn bump(self:&Cell[Item]!)void{self.value++;}}fn main()void{var c:Cell[bool]=Cell{value:true};c.bump();}", "S06"),
    "borrowed_bytes_write": ('fn main()void{var data=b"AB";var reference=&data!;reference[0]++;}', "S07"),
    "borrowed_array_bounds": ("fn bump(values:&[i32;2]!)void{values[2]++;}fn main()void{}", "S06"),
    "borrowed_array_bool": ("fn bump(values:&[bool;2]!)void{values[0]++;}fn main()void{}", "S06"),
}

VALID = '''fn update(values:&[f64;2]!)void{values[0]++;values[1]+=values[0];}
fn check_borrowed_array()bool{var a=[1.5,2.5];update(&a!);ret a[0]==2.5&&a[1]==5.0;}
fn bump[T](value:T)T{value++;ret value;}
fn outer[U](value:U)U{ret bump[U](value);}
fn add[T](value:T,offset:T)T{value+=offset;ret value;}
fn rhs[Marker,U](value:i32,offset:U)i32{value+=offset;ret value;}
type Scalar=f64;
struct Cell[T]{value:T,}
impl Cell[Item]{fn bump(self:&Cell[Item]!)void{self.value++;}}
fn check_generic()bool{ret outer[i32](1)==2&&outer[f64](1.5)==2.5&&add[i32](1,2)==3&&add[Scalar](1.5,2.5)==4.0&&rhs[bool,i64](1,2)==3;}
fn check_impl()bool{var c:Cell[f64]=Cell{value:1.5};c.bump();ret c.value==2.5;}
fn add_reference[T](value:T,offset:&i32)T{value+=offset;ret value;}
fn check_references()bool{var offset=2;var reference=&offset;var n=1;n+=reference;ret n==3&&add_reference[i32](1,reference)==3;}
unsafe fn offset[T](value:T,amount:i32)T{value+=amount;value-=1;ret value;}
fn check_pointer()bool{unsafe{var p:*u8=0;ret offset[*u8](p,4)==3;}}
fn slice_read(...items:i32)i32{var reference=&items;ret reference[1];}
fn slice_update(...items:f64)f64{var reference=&items!;reference[0]++;reference[1]+=reference[0];ret reference[1];}
fn check_borrowed_slice()bool{ret slice_read(1,2)==2&&slice_update(1.5,2.5)==5.0;}
fn check_borrowed_bytes()bool{var data=b"AB";var reference=&data;ret reference[1]==66;}
fn main()i32{if !check_borrowed_array(){ret 1;}if !check_generic(){ret 2;}if !check_impl(){ret 3;}if !check_references(){ret 4;}if !check_pointer(){ret 5;}if !check_borrowed_slice(){ret 6;}if !check_borrowed_bytes(){ret 7;}ret 0;}
'''


def run(command, root, expected=0):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    assert result.returncode == expected, (command, result.returncode, result.stdout)
    return result.stdout


def project(root, source):
    root.mkdir()
    (root / "quazi.toml").write_text('[package]\nname="numeric_mutations"\nstd=false\ncrash_handler=false\n[build]\nentry="main.qz"\n')
    (root / "main.qz").write_text(source)


def check(qz, root, before):
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    for name, (source, code) in INVALID.items():
        work = root / name
        project(work, source)
        for arguments in [["check"], ["build", *common], ["build", *common, "-i"],
                          ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *arguments], cwd=work, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and code in result.stdout, (name, arguments, result.stdout)
    print(f"PASS numeric mutations: {len(INVALID)} rejected check/native/QZI/Windows COFF")

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
    portable = emission / "native"
    run([str(qz), "build", str(artifact), *common, "-o", str(portable)], emission)
    run([str(portable)], emission)
    coff = emission / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS numeric mutations: 7 valid checks native/source-removed QZI/Windows COFF")

    traps = {
        "bytes": 'fn read(index:i32)i32{var data=b"AB";var reference=&data;ret reference[index] as i32;}fn main()i32{ret read(2);}',
        "slice_read": 'fn read(index:i32,...items:i32)i32{var reference=&items;ret reference[index];}fn main()i32{ret read(2,1,2);}',
        "slice_update": 'fn update(index:i32,...items:i32)i32{var reference=&items!;reference[index]++;ret 0;}fn main()i32{ret update(2,1,2);}',
    }
    for name, source in traps.items():
        work = root / f"bounds_{name}"
        project(work, source)
        binary = work / "native"
        run([str(qz), "build", *common, "-o", str(binary)], work)
        run([str(binary)], work, -4)
        artifact = work / "program.qzi"
        run([str(qz), "build", *common, "-i", "-o", str(artifact)], work)
        (work / "main.qz").unlink()
        binary = emission / f"bounds_{name}"
        run([str(qz), "build", str(artifact), *common, "-o", str(binary)], emission)
        run([str(binary)], emission, -4)
    print("PASS borrowed builtin bounds: 3 native/source-removed QZI traps")

    if before:
        invalid = root / "invalid_cache"
        project(invalid, INVALID["struct_increment"][0])
        options = ["build", "--no-color", "--no-unicode", "-o", str(invalid / "native")]
        # Compile only. The previous compiler emits invalid owner arithmetic;
        # this old executable must never run.
        run([str(before), *options], invalid)
        result = subprocess.run([str(qz), *options], cwd=invalid, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode != 0 and "S06" in result.stdout, result.stdout
        cache = root / "valid_cache"
        cache_source = VALID.split("fn bump[T]")[0] + "fn main()i32{if check_borrowed_array(){ret 0;}ret 1;}"
        project(cache, cache_source)
        binary = cache / "native"
        options = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
        run([str(before), *options], cache)
        # Initialized array slots stay within their valid reference. The old
        # missing float annotation produces a wrong number, not an invalid ptr.
        run([str(binary)], cache, 1)
        for expected in ["miss", "hit"]:
            output = run([str(qz), *options], cache)
            assert f"cache lookup {expected}" in " ".join(output.split()), output
            run([str(binary)], cache)
        print("PASS numeric mutation caches: reject old unsafe source, rebuild old wrong float result, correct exact hit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-numeric-mutations-") as directory:
        check(args.qz.resolve(), Path(directory), args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
