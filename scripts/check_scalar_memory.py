#!/usr/bin/env python3
"""Check raw scalar widths, float updates, VM references and portable artifacts."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PRELUDE = '''@intrinsic("quazi.malloc") fn allocate(size: usize) *u8;
@intrinsic("quazi.free") fn release(pointer: *u8) void;
@repr(C) struct Cell { value: f32, guard: u32 }
@repr(C) struct Narrow { first:i8, second:u8, tail:u16 }
type Scalar = f32;
unsafe fn read(pointer: *Scalar) f32 { ret *(pointer); }
unsafe fn write(pointer: *Scalar, value: f32) void { *pointer = value; }
unsafe fn add(pointer: *Scalar, value: f32) void { *pointer += value; }
unsafe fn post_inc(pointer: *Scalar) f32 { ret (*pointer)++; }
unsafe fn pre_inc(pointer: *Scalar) f32 { ret ++(*pointer); }
unsafe fn post_dec(pointer: *Scalar) f32 { ret (*pointer)--; }
unsafe fn pre_dec(pointer: *Scalar) f32 { ret --(*pointer); }
unsafe fn generic_store[T](pointer:*T,value:T)void{*pointer=value;}
unsafe fn generic_read[T](pointer:*T)T{ret *pointer;}
'''

CASES = {
    "raw_load": "(*cell).value=1.5;ret read(memory as *Scalar)==1.5;",
    "raw_store": "write(memory as *Scalar,1.5);ret (*cell).value==1.5&&(*cell).guard==123456;",
    "raw_compound": "(*cell).value=1.5;add(memory as *Scalar,2.5);ret (*cell).value==4.0&&(*cell).guard==123456;",
    "raw_post_inc": "(*cell).value=1.5;var old=post_inc(memory as *Scalar);ret old==1.5&&(*cell).value==2.5&&(*cell).guard==123456;",
    "raw_pre_inc": "(*cell).value=1.5;var value=pre_inc(memory as *Scalar);ret value==2.5&&(*cell).value==2.5&&(*cell).guard==123456;",
    "raw_post_dec": "(*cell).value=2.5;var old=post_dec(memory as *Scalar);ret old==2.5&&(*cell).value==1.5&&(*cell).guard==123456;",
    "generic_raw": "generic_store[f32](memory as *f32,1.5);ret generic_read[f32](memory as *f32)==1.5&&(*cell).guard==123456;",
    "narrow_signed": "var row=memory as *Narrow;(*row).second=77;(*row).tail=1234;var p=memory as *i8;*p=-7;var before=(*p)++;ret before==-7&&*p==-6&&(*row).second==77&&(*row).tail==1234&&(*cell).guard==123456;",
    "raw_pre_dec": "(*cell).value=2.5;var value=pre_dec(memory as *Scalar);ret value==1.5&&(*cell).value==1.5&&(*cell).guard==123456;",
}

CONTROLS = '''fn increment[T](value:T)T{var local=value;local++;ret local;}
fn generic_float()bool{ret increment[f32](1.5)==2.5&&increment[f64](1.5)==2.5;}
fn local_float()bool {
    var value:f64=1.5;
    var before=value++;
    var after=++value;
    var before_dec=value--;
    var after_dec=--value;
    ret before==1.5&&after==3.5&&before_dec==3.5&&after_dec==1.5&&value==1.5;
}
fn reference_float()bool {
    var value:f32=1.5;
    var reference=&value!;
    var before=(*reference)++;
    var after=++(*reference);
    ret before==1.5&&after==3.5&&*reference==3.5;
}
@repr(C) struct Fields { narrow:f32, wide:f64 }
fn field_float()bool {
    var value=Fields{narrow:1.5,wide:2.5};
    var before=value.narrow++;
    var after=++value.wide;
    ret before==1.5&&after==3.5&&value.narrow==2.5&&value.wide==3.5;
}
'''


def source():
    functions = [PRELUDE, CONTROLS]
    conditions = []
    for index, (name, body) in enumerate(CASES.items(), 1):
        functions.append(f'''unsafe fn {name}_inner(memory:*u8)bool {{
            var cell=memory as *Cell;
            (*cell).guard=123456;
            {body}
        }}
        fn {name}()bool {{ unsafe {{
            var memory=allocate(16);
            var result={name}_inner(memory);
            release(memory);
            ret result;
        }} }}''')
        conditions.append(f"if !{name}(){{ret {index};}}")
    for index, name in enumerate(["local_float", "reference_float", "field_float", "generic_float"], len(CASES)+1):
        conditions.append(f"if !{name}(){{ret {index};}}")
    return "\n".join(functions) + "\nfn main()i32{" + "\n".join(conditions) + "ret 0;}"


def run(command, root, expected=0):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    assert result.returncode == expected, (command, result.returncode, result.stdout)
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    with tempfile.TemporaryDirectory(prefix="quazi-scalar-memory-") as directory:
        workspace = Path(directory)
        root = workspace / "source"
        root.mkdir()
        emission = workspace / "artifact"
        emission.mkdir()
        (root / "quazi.toml").write_text('[package]\nname="scalar_memory"\nstd=false\ncrash_handler=false\n[build]\nentry="main.qz"\n')
        (root / "main.qz").write_text(source())
        run([qz, "check"], root)
        binary = root / "native"
        run([qz, "build", *common, "-o", str(binary)], root)
        run([str(binary)], root)
        artifact = root / "program.qzi"
        run([qz, "build", *common, "-i", "-o", str(artifact)], root)
        (root / "main.qz").unlink()
        portable = emission / "native"
        run([qz, "build", str(artifact), *common, "-o", str(portable)], emission)
        run([str(portable)], emission)
        coff = emission / "program.obj"
        run([qz, "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], emission)
        assert coff.read_bytes()[:2] == b"\x64\x86"
        if args.before_qz:
            # Every baseline access is inside a 16-byte allocation; the first
            # f32 load observes initialized value+guard bits and returns false.
            (root / "main.qz").write_text(source())
            options = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
            run([str(args.before_qz.resolve()), *options], root)
            run([str(binary)], root, 1)
            for expected in ["miss", "hit"]:
                output = run([qz, *options], root)
                assert f"cache lookup {expected}" in " ".join(output.split()), output
                run([str(binary)], root)
            print("PASS scalar memory cache: previous wrong result, rebuild, correct exact hit")
        print(f"PASS scalar memory: {len(CASES)+4} native/source-removed QZI/Windows COFF")


if __name__ == "__main__":
    main()
