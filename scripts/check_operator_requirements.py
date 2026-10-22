#!/usr/bin/env python3
"""Check bitwise, shift and ordering requirements across source and artifacts."""
import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
from check_numeric_mutations import project, run

INVALID = {}
for operator, name in [("&", "and"), ("|", "or"), ("^", "xor"), ("<<", "shl"), (">>", "shr")]:
    INVALID[f"owner_{name}"] = (f"struct Owner{{value:i32,}}fn main()void{{var a=Owner{{value:1}};var b=Owner{{value:2}};var result=a{operator}b;}}", "S06")
    INVALID[f"float_{name}"] = (f"fn main()void{{var a=1.5;var result=a{operator}a;}}", "S06")
    INVALID[f"generic_float_{name}"] = (f"fn calc[T](a:T,b:T)T{{ret a{operator}b;}}fn main()void{{calc[i32](5,2);calc[f64](1.5,2.5);}}", "S06")
    INVALID[f"missing_{name}"] = (f"enum E{{One,}}fn main()void{{var a=E.One;var result=a{operator}a;}}", "S14")
for operator, name in [("<", "lt"), ("<=", "le"), (">", "gt"), (">=", "ge")]:
    INVALID[f"generic_owner_{name}"] = (f"struct Owner{{value:i32,}}fn calc[T](a:T,b:T)bool{{ret a{operator}b;}}fn main()void{{calc[i32](1,2);calc[Owner](Owner{{value:1}},Owner{{value:2}});}}", "S06")
    INVALID[f"generic_bool_{name}"] = (f"fn calc[T](a:T,b:T)bool{{ret a{operator}b;}}fn main()void{{calc[bool](true,false);}}", "S06")
    INVALID[f"mixed_numbers_{name}"] = (f"fn calc[A,B](a:A,b:B)bool{{ret a{operator}b;}}fn main()void{{calc[i32,f64](1,2.5);}}", "S06")
INVALID.update({
    "bool_shl": ("fn calc[T](a:T,b:T)T{ret a<<b;}fn main()void{calc[bool](true,false);}", "S06"),
    "bool_shr": ("fn calc[T](a:T,b:T)T{ret a>>b;}fn main()void{calc[bool](true,false);}", "S06"),
    "generic_string_binder": ("struct Owner{value:i32,}fn compare[String](a:String,b:String)bool{ret a<b;}fn main()void{compare[Owner](Owner{value:1},Owner{value:2});}", "S06"),
    "nested_exclusive_views": ('fn main()void{var text:str="a";var first=&text!;var second=&first!;var result=second<"b";}', "S06"),
    "generic_ref_bits": ("fn calc[T](a:T,b:&T)T{ret a&b;}fn main()void{}", "S06"),
})

VALID = '''fn bitand[T](a:T,b:T)T{ret a&b;}
fn bitor[T](a:T,b:T)T{ret a|b;}
fn bitxor[T](a:T,b:T)T{ret a^b;}
fn shl[T](a:T,b:T)T{ret a<<b;}
fn shr[T](a:T,b:T)T{ret a>>b;}
fn lt[A,B](a:A,b:B)bool{ret a<b;}
fn le[T](a:T,b:T)bool{ret a<=b;}
fn gt[T](a:T,b:T)bool{ret a>b;}
fn ge[T](a:T,b:T)bool{ret a>=b;}
fn and_ref[T](a:&bool,b:T)bool{ret a&b;}
fn or_ref[T](a:T,b:&bool)bool{ret a|b;}
fn shift_ref[U](a:&i64,b:U)i64{ret a>>b;}
fn less_ref[T](a:&f64,b:T)bool{ret a<b;}
fn less_view[T](a:&str!,b:T)bool{ret a<b;}
fn compare[String](a:String,b:String)bool{ret a<b;}
fn bits()bool{ret bitand[i32](5,3)==1&&bitor[i32](5,3)==7&&bitxor[i32](5,3)==6;}
fn bools()bool{var value=true;var reference=&value;ret bitand[bool](true,true)&&!bitand[bool](true,false)&&bitor[bool](false,true)&&bitxor[bool](true,false)&&and_ref[bool](reference,true)&&or_ref[bool](false,reference);}
fn shifts()bool{var value:i64=-8;var reference=&value;ret shl[i64](3,2)==12&&shr[i64](-8,2)==-2&&shr[u64](18446744073709551615,1)==9223372036854775807&&shift_ref[i32](reference,2)==-2;}
fn ordering()bool{var value=1.5;var reference=&value;ret lt[i32,u64](1,2)&&le[f64](2.5,2.5)&&gt[u64](18446744073709551615,9223372036854775807)&&ge[i32](2,2)&&less_ref[f64](reference,2.5)&&compare[i32](1,2);}
fn views()bool{var value:str="a";var exclusive=&value!;ret lt[str,str]("a","b")&&less_view[str](exclusive,"b");}
fn main()i32{if !bits(){ret 1;}if !bools(){ret 2;}if !shifts(){ret 3;}if !ordering(){ret 4;}if !views(){ret 5;}ret 0;}
'''
OWNED = '''fn compare[A,B](a:A,b:B)bool{ret a<b;}
fn compare_name[String](a:String,b:String)bool{ret a<b;}
fn main()i32{if !compare_name[i32](1,2){ret 3;}var left=String.from("a");var right=String.from("b");if !compare[String,String](left,right){ret 1;}var owner=String.from("a");if !compare[String,str](owner,"b"){ret 2;}ret 0;}
'''


def artifacts(qz, root, name, source, std=False):
    work = root / name
    project(work, source)
    if std:
        manifest = work / "quazi.toml"
        manifest.write_text(manifest.read_text().replace("std=false\n", ""))
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    run([str(qz), "check"], work)
    binary = work / "native"
    run([str(qz), "build", *common, "-o", str(binary)], work)
    run([str(binary)], work)
    artifact = work / "program.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], work)
    (work / "main.qz").unlink()
    emission = root / f"{name}_artifact"
    emission.mkdir()
    binary = emission / "native"
    run([str(qz), "build", str(artifact), *common, "-o", str(binary)], emission)
    run([str(binary)], emission)
    coff = emission / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"


def check(qz, root, before):
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    for name, (source, code) in INVALID.items():
        work = root / name
        project(work, source)
        for options in [["check"], ["build", *common], ["build", *common, "-i"], ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and code in result.stdout, (name, options, result.stdout)
    print(f"PASS operator requirements: {len(INVALID)} invalid sources rejected check/native/QZI/Windows COFF", flush=True)
    artifacts(qz, root, "values", VALID)
    artifacts(qz, root, "owned_strings", OWNED, std=True)
    print("PASS operator requirements: 5 value groups and 2 owned-string checks native/source-removed QZI/Windows COFF", flush=True)
    if before:
        invalid = root / "invalid_cache"
        project(invalid, INVALID["owner_and"][0])
        options = ["build", "--no-color", "--no-unicode", "-o", str(invalid / "native")]
        # Compile only: never run the old owning-handle arithmetic.
        run([str(before), *options], invalid)
        result = subprocess.run([str(qz), *options], cwd=invalid, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode != 0 and "S06" in result.stdout, result.stdout
        work = root / "valid_cache"
        project(work, 'fn and_ref[T](a:&bool,b:T)bool{ret a&b;}fn main()i32{var value=true;if and_ref[bool](&value,true){ret 0;}ret 1;}')
        binary = work / "native"
        options = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
        run([str(before), *options], work)
        # The old code masks an aligned valid slot address with1; no invalid
        # pointer dereference or owning result executes. It deterministically
        # returns false rather than loading the stored true value.
        run([str(binary)], work, 1)
        for expected in ["miss", "hit"]:
            output = run([str(qz), *options], work)
            assert f"cache lookup {expected}" in " ".join(output.split()), output
            run([str(binary)], work)
        print("PASS operator caches: old owner source rejected; Boolean reference load correct on rebuild/new hit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-operators-") as directory:
        check(args.qz.resolve(), Path(directory), args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
