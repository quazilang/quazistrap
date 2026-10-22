#!/usr/bin/env python3
"""Verify eager value snapshots, stable places, ABI artifacts and cache upgrades."""
import argparse
import subprocess
import tempfile
from pathlib import Path

CASES = {
    "binary": ("fn binary()i32{var value=1;ret value+(value=2);}", "binary()==3"),
    "condition": ("fn condition()i32{var value=1;if value<(value=2){ret 3;}ret 4;}", "condition()==3"),
    "call": ("fn first(a:i32,b:i32)i32{ret a;}fn call()i32{var value=1;ret first(value,(value=2));}", "call()==1"),
    "compound": ("fn compound()i32{var value=1;value+=(value=2);ret value;}", "compound()==3"),
    "array_place": ("fn array_place()i32{var values=[1,2];var index=0;values[index]=(index=1);ret values[0]*10+values[1];}", "array_place()==12"),
    "struct": ("struct Pair{first:i32,second:i32,}fn structure()i32{var value=1;var pair=Pair{first:value,second:(value=2)};ret pair.first;}", "structure()==1"),
    "enum": ("enum Values{Pair(i32,i32),}fn enumeration()i32{var value=1;var pair:Values=Values.Pair(value,(value=2));ret match pair{Pair(first,_)=>first,};}", "enumeration()==1"),
    "range_start": ("fn range_start()i32{var start=1;for index:start..(start=3){ret index;}ret 0;}", "range_start()==1"),
    "range_end": ("fn range_end()i32{var end=3;var count=0;for index:0..end{end=0;count+=1;}ret count;}", "range_end()==3"),
    "array_literal": ("fn array_literal()i32{var value=1;var values=[value,(value=2)];ret values[0];}", "array_literal()==1"),
    "named": ("fn named()i32{var x=1;ret first(b=(x=2),a=x);}", "named()==1"),
    "generic": ("fn generic_first[T](a:T,b:T)T{ret a;}fn generic()i32{var x=1;ret generic_first[i32](x,(x=2));}", "generic()==1"),
    "indirect": ("fn indirect()i32{var f:fn(i32,i32)i32=first;var x=1;ret f(x,(x=2));}", "indirect()==1"),
    "static": ("struct Factory{value:i32,}impl Factory{fn choose(a:i32,b:i32)i32{ret a;}}fn static_call()i32{var x=1;ret Factory.choose(x,(x=2));}", "static_call()==1"),
    "method": ("struct Receiver{value:i32,}impl Receiver{fn choose(self:&Receiver,a:i32,b:i32)i32{ret a;}}fn method_call()i32{var r=Receiver{value:0};var x=1;ret r.choose(x,(x=2));}", "method_call()==1"),
    "floating": ("fn floating()bool{var x:f64=1.0;ret x+(x=2.0)==3.0;}", "floating()"),
    "string_view": ('fn view_first(a:str,b:str)str{ret a;}fn string_view()bool{var s="a";ret view_first(s,(s="b"))=="a";}', "string_view()"),
    "array_match": ("fn array_match(flag:bool)i32{var values=match flag{true=>[1,2],false=>[3,4],};ret values[1];}", "array_match(true)==2&&array_match(false)==4"),
    "slice_alias": ("fn slice_alias(...values:i32)i32{var alias=values;ret (alias).len() as i32*10+alias[1];}", "slice_alias(1,2,3)==32"),
    "untyped_enum": ("enum Wrap{Value(i32),Empty,}fn wrapped_value(value:Wrap)i32{ret match value{Value(x)=>x,_=>0,};}fn untyped_enum()i32{var value=Wrap.Value(7);ret wrapped_value(value)+wrapped_value(Wrap.Empty);}", "untyped_enum()==7"),
    "compound_field": ("struct Number{value:i32,}fn compound_field()i32{var n=Number{value:1};n.value+=(n.value=2);ret n.value;}", "compound_field()==3"),
    "compound_index": ("fn compound_index()i32{var a=[1,2];var i=0;a[i]+=(i=1);ret a[0]*10+a[1];}", "compound_index()==22"),
    "raw_place": ("unsafe fn raw_write(p:*i32,q:*i32)void{*p=*(p=q);}fn raw_place()i32{var left:i32=1;var right:i32=2;unsafe{raw_write(&left,&right);}ret left*10+right;}", "raw_place()==22"),
    "owned_fields": ("struct Tracked{trace:*i32,digit:i32,}impl Tracked{fn free(self:Tracked)void{unsafe{*self.trace=*self.trace*10+self.digit;}}}struct OwnedPair{first:Tracked,second:Tracked,}unsafe fn tracked(p:*i32,d:i32)Tracked{ret Tracked{trace:p,digit:d};}unsafe fn build_pair(p:*i32)void{var owner=tracked(p,1);var pair=OwnedPair{first:owner,second:(owner=tracked(p,2))};}fn owned_fields()i32{var trace:i32=0;unsafe{build_pair(&trace);}ret trace;}", "owned_fields()==21"),
    "owned_receiver": ("impl Tracked{fn accept(self:Tracked,scalar:i32)void{}}unsafe fn consume_replaced(p:*i32)void{var owner=tracked(p,1);owner.accept((owner=tracked(p,2)).digit);}fn owned_receiver()i32{var trace:i32=0;unsafe{consume_replaced(&trace);}ret trace;}", "owned_receiver()==12"),
    "c_callback": ('@repr(C) type Callback=fn(i32,i32)i32;@export("native_choose")pub fn choose_callback(a:i32,b:i32)i32{ret a;}fn c_callback()i32{var x=1;unsafe{var f:Callback=choose_callback;ret f(x,(x=2));}}', "c_callback()==1"),
    "flexible_float": ('@repr(C) struct Packet{length:u32,data:[f32;..],}@intrinsic("quazi.malloc")fn allocate(size:usize)*u8;@intrinsic("quazi.free")fn release(ptr:*u8)void;fn flexible_float()bool{unsafe{var memory=allocate(16);var packet=memory as *Packet;(*packet).data[0]=1.5;(*packet).data[0]+=2.5;var result=(*packet).data[0]==4.0;release(memory);ret result;}}', "flexible_float()"),
}

INVALID = {
    "pending_field_owner": "struct Owner{value:i32,}fn make(v:i32)Owner{ret Owner{value:v};}fn main()void{var owner=make(1);owner.value=(owner=make(2)).value;}",
    "pending_fn_callee": "fn one(x:i32)i32{ret 1;}fn two(x:i32)i32{ret 2;}fn main()i32{var f:fn(i32)i32=one;ret f((f=two)(0));}",
    "pending_string_compare": 'fn main()void{var s=String.from("a");var equal=s.as_str()==(s=String.from("b")).as_str();}',
    "pending_string_concat": 'fn main()void{var s=String.from("a");var text=s.as_str()+(s=String.from("b")).as_str();}',
    "pending_string_index": 'fn main()i32{var s=String.from("a");ret s.as_str()[(s=String.from("b")).len()-1] as i32;}',
    "pending_match_owner": "enum E{Value(i32),Other,}fn main()i32{var e:E=E.Value(1);ret match e{Value(x) if((e=E.Value(2))==E.Other)=>x,Value(y)=>y,_=>0,};}",
}


def run(command, root, expected=0):
    result = subprocess.run(
        command, cwd=root, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, timeout=60,
    )
    assert result.returncode == expected, (
        f"{command!r}: {result.returncode}, expected {expected}\n{result.stdout}"
    )
    return result.stdout


def project(root, source):
    root.mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="snapshots"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="main.qz"\n'
    )
    (root / "main.qz").write_text(source)


def source():
    functions = []
    checks = []
    for index, (name, (body, condition)) in enumerate(CASES.items(), 1):
        functions.append(body + f"\nfn check_{name}()bool{{ret {condition};}}")
        checks.append(f"if !check_{name}(){{ret {index};}}")
    return "\n".join(functions) + "\nfn main()i32{" + "\n".join(checks) + "ret 0;}"


def check(qz, root, before):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    work = root / "source"
    project(work, source())
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
    run([str(qz), "build", str(artifact), *common, "-c", "--target",
         "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print(f"PASS operand snapshots: {len(CASES)} checks native/source-removed QZI/Windows COFF")

    for name, body in INVALID.items():
        invalid = root / name
        project(invalid, body)
        if "String" in body:
            manifest = invalid / "quazi.toml"
            manifest.write_text(manifest.read_text().replace("std=false", "std=true"))
        commands = [
            ["check"], ["build", *common], ["build", *common, "-i"],
            ["build", *common, "-c", "--target", "x86_64-windows"],
        ]
        for command in commands:
            result = subprocess.run(
                [str(qz), *command], cwd=invalid, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60,
            )
            assert result.returncode != 0 and "S10" in result.stdout, (
                name, command, result.stdout,
            )
    print(f"PASS pending storage: {len(INVALID)} rejected check/native/QZI/Windows COFF")

    if before:
        cache = root / "cache"
        project(cache, CASES["call"][0] + "fn main()i32{ret call()-1;}")
        binary = cache / "native"
        args = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
        run([str(before), *args], cache)
        run([str(binary)], cache, 1)
        for expected in ["miss", "hit"]:
            output = run([str(qz), *args], cache)
            assert f"cache lookup {expected}" in " ".join(output.split()), output
            run([str(binary)], cache)
        print("PASS operand snapshots cache: old wrong result, rebuild, new exact hit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-snapshots-") as directory:
        check(args.qz.resolve(), Path(directory),
              args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
