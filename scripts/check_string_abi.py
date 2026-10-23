#!/usr/bin/env python3
"""Check selected runtime String identity without executing counterfeit owners."""
import argparse
import os
import subprocess
import sys
sys.dont_write_bytecode = True
import tempfile
from contextlib import contextmanager
from pathlib import Path

from check_numeric_mutations import project, run
from check_operator_requirements import artifacts

INVALID = {}
for name, field in [("integer", "value:i32"), ("pointer", "data:*u8"), ("view", "data:str")]:
    declaration = f"pub struct String{{{field},}}"
    INVALID[f"{name}_direct"] = declaration + "fn compare(a:String,b:String)bool{ret a<b;}"
    INVALID[f"{name}_alias"] = declaration + "type Text=String;fn compare(a:Text,b:Text)bool{ret a<b;}"
    INVALID[f"{name}_generic"] = declaration + "fn compare[T](a:T,b:T)bool{ret a<b;}fn force(a:String,b:String)bool{ret compare[String](a,b);}"
INVALID["owner_view"] = "struct Owner{value:i32,}fn view(owner:&Owner)void{owner.as_str();}"
INVALID["owner_string"] = "struct Owner{value:i32,}fn view(owner:&Owner)void{owner.as_string();}"

FORMAT_INVALID = {
    "format_owner": "fn main()void{sink(Owner{value:1});}",
    "format_owner_ref": "fn main()void{var owner=Owner{value:1};sink(&owner);}",
    "format_pointer": "unsafe fn main()void{var pointer:*u8=0;sink(pointer);}",
    "format_generic_owner": "fn display[T](value:T)void{sink(value);}fn main()void{display[Owner](Owner{value:1});}",
    "format_generic_owner_ref": "fn display[T](value:&T)void{sink(value);}fn main()void{var owner=Owner{value:1};display[Owner](&owner);}",
    "format_generic_pointer": "fn display[T](value:T)void{sink(value);}unsafe fn main()void{var pointer:*u8=0;display[*u8](pointer);}",
}

VALID = '''type Text=String;
fn less[T](a:T,b:T)bool{ret a<b;}
fn less_ref(a:&String,b:&String)bool{ret a<b;}
fn parse_ref(a:&String)i32{ret a.parse[i32]().unwrap();}
fn comparison()bool{var a=String.from("alpha");var b=String.from("beta");if !(a<b){ret false;}if !less_ref(&a,&b){ret false;}ret true;}
fn generic_comparison()bool{var a:Text=String.from("alpha");var b=String.from("beta");ret less[Text](a,b);}
fn formatted()bool{var owner=String.from("value");var result=fmt.format("[{}]",owner);ret result.as_str()=="[value]";}
fn parsed()bool{var owner=String.from("42");ret parse_ref(&owner)==42;}
fn main()i32{if !comparison(){ret 1;}if !generic_comparison(){ret 2;}if !formatted(){ret 3;}if !parsed(){ret 4;}ret 0;}
'''


@contextmanager
def selected_prelude(path):
    previous = os.environ.get("QUAZI_PRELUDE_ROOT")
    os.environ["QUAZI_PRELUDE_ROOT"] = str(path)
    try:
        yield
    finally:
        if previous is None:
            os.environ.pop("QUAZI_PRELUDE_ROOT", None)
        else:
            os.environ["QUAZI_PRELUDE_ROOT"] = previous


def custom_runtime(qz, root):
    for declaration in ["String", "Text"]:
        prelude = root / f"prelude_{declaration}"
        (prelude / "src").mkdir(parents=True)
        alias = " as String" if declaration == "Text" else ""
        (prelude / "src" / "mod.qz").write_text(f"pub import ./text.{declaration}{alias};")
        (prelude / "src" / "text.qz").write_text(f"pub struct {declaration}{{tag:i32,data:str,}}impl {declaration}{{pub fn marker()i32{{ret 0;}}}}")
        source = '''fn less[T](a:T,b:T)bool{ret a<b;}
fn main()i32{var a=String{tag:99,data:"alpha"};var b=String{tag:11,data:"beta"};if !(a<b){ret 1;}if !less[String](a,b){ret 2;}ret String.marker();}'''
        with selected_prelude(prelude):
            artifacts(qz, root, f"checked_offset_{declaration}", source, std=True)
    print("PASS String ABI: selected nonzero data offset and renamed export native/source-removed QZI/COFF", flush=True)


def runtime_cache(qz,root):
    work=root / "runtime_cache"
    project(work, 'fn main()i32{var a=String{tag:7,data:"alpha"};var b=String{tag:9,data:"beta"};if !(a<b){ret 9;}ret String.marker();}')
    manifest=work / "quazi.toml"
    manifest.write_text(manifest.read_text().replace("std=false\n", ""))
    roots=[]
    for number,fields in [(1,"data:str,tag:i32"),(2,"tag:i32,data:str")]:
        prelude=root / f"cache_prelude_{number}"
        (prelude / "src").mkdir(parents=True)
        (prelude / "src/mod.qz").write_text(f"pub struct String{{{fields},}}impl String{{pub fn marker()i32{{ret {number};}}}}")
        roots.append((prelude,number))
    binary=work / "native"
    for prelude,expected in [*roots,roots[0]]:
        with selected_prelude(prelude):
            for state in ["miss","hit"]:
                output=run([str(qz),"build","--no-color","--no-unicode","-o",str(binary)],work)
                assert f"cache lookup {state}" in " ".join(output.split()),output
                run([str(binary)],work,expected)
    print("PASS String ABI: runtime root/layout switches invalidate exact/reusable caches and new hits execute correct runtime",flush=True)


def check(qz, root, before=None):
    common = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
    for name, source in INVALID.items():
        work = root / name
        project(work, source + "fn main()void{}")
        for options in [["check"], ["build", *common], ["build", *common, "-i"],
                        ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and "S06" in result.stdout, (name, options, result.stdout)
    for name, source in [
        ("qualified", "import fake;fn compare(a:fake.String,b:fake.String)bool{ret a<b;}fn main()void{}"),
        ("import_alias", "import fake.String as Text;fn compare(a:Text,b:Text)bool{ret a<b;}fn main()void{}"),
    ]:
        work = root / name
        project(work, source)
        (work / "fake.qz").write_text("pub struct String{data:str,}")
        for options in [["check"], ["build", *common], ["build", *common, "-i"],
                        ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and "S06" in result.stdout, (name, options, result.stdout)
    work = root / "forged_import_of_runtime_name"
    project(work, "import ./fake.String as Forged;fn compare(a:Forged,b:Forged)bool{ret a<b;}fn main()void{}")
    manifest = work / "quazi.toml"
    manifest.write_text(manifest.read_text().replace("std=false\n", ""))
    (work / "fake.qz").write_text("pub struct Marker{value:i32,}")
    for options in [["check"], ["build", *common], ["build", *common, "-i"],
                    ["build", *common, "-c", "--target", "x86_64-windows"]]:
        result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode != 0, ("forged_import_of_runtime_name", options, result.stdout)
    for name, source in [
        ("unproven_same_selector", "import ./fake.String;fn compare(a:String,b:String)bool{ret a<b;}fn main()void{}"),
        ("nested_owned_reference", "fn compare(a:&&String,b:&&String)bool{ret a<b;}fn main()void{}"),
        ("nested_exclusive_owned_reference", "fn compare(a:&&String!,b:&&String!)bool{ret a<b;}fn main()void{}"),
    ]:
        work = root / name
        project(work,source)
        manifest=work / "quazi.toml"
        manifest.write_text(manifest.read_text().replace("std=false\n", ""))
        (work / "fake.qz").write_text("pub struct Marker{value:i32,}")
        for options in [["check"], ["build", *common], ["build", *common, "-i"], ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result=subprocess.run([str(qz),*options],cwd=work,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=60)
            assert result.returncode != 0, (name,options,result.stdout)
    print("PASS String ABI: exact selector and nested owner-reference regressions rejected in all4 modes",flush=True)
    print(f"PASS String ABI: {len(INVALID) + 3} counterfeit/reference sources rejected check/native/QZI/COFF", flush=True)
    control = root / "valid_runtime_import_alias"
    project(control, "import ./gateway.String as Renamed;fn compare(a:Renamed,b:Renamed)bool{ret a<b;}fn main()void{}")
    manifest = control / "quazi.toml"
    manifest.write_text(manifest.read_text().replace("std=false\n", ""))
    (control / "gateway.qz").write_text("pub import prelude.String;")
    run([str(qz), "check"], control)
    for name, source, extra_files in [
        ("forged_nested_runtime_name", "import ./fake.string.String as Forged;fn compare(a:Forged,b:Forged)bool{ret a<b;}fn main()void{}",
         {"fake/string.qz": "pub struct Marker{value:i32,}"}),
        ("runtime_import_alias_collision", "pub struct Owner{value:i32,}import ./gateway.String as Owner;fn compare(a:Owner,b:Owner)bool{ret a<b;}fn main()void{}",
         {"gateway.qz": "pub import prelude.String;"}),
    ]:
        work = root / name
        project(work, source)
        manifest = work / "quazi.toml"
        manifest.write_text(manifest.read_text().replace("std=false\n", ""))
        for filename, content in extra_files.items():
            path = work / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        for options in [["check"], ["build", *common], ["build", *common, "-i"],
                        ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0, (name, options, result.stdout)
    print("PASS String ABI: nested forged runtime import and conflicting runtime alias rejected check/native/QZI/COFF", flush=True)
    for name, body in FORMAT_INVALID.items():
        work = root / name
        project(work, "struct Owner{value:i32,}@format fn sink(...parts:str)void{}" + body)
        for options in [["check"], ["build", *common], ["build", *common, "-i"],
                        ["build", *common, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0 and "S06" in result.stdout, (name, options, result.stdout)
    print(f"PASS String ABI: {len(FORMAT_INVALID)} unsupported format sources rejected check/native/QZI/COFF", flush=True)
    if before:
        work = root / "counterfeit_cache"
        project(work, 'struct String{value:i32,}fn compare[T](a:T,b:T)bool{ret a<b;}fn main()void{compare[String](String{value:1},String{value:2});}')
        options = ["build", "--no-color", "--no-unicode", "-o", str(work / "never_execute")]
        # Compile only to seed the old source cache. Never execute counterfeit owners.
        run([str(before), *options], work)
        result = subprocess.run([str(qz), *options], cwd=work, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode != 0 and "S06" in result.stdout, result.stdout
        print("PASS String ABI: counterfeit cached source rejected without executing its body", flush=True)
    runtime_cache(qz, root)
    custom_runtime(qz, root)
    artifacts(qz, root, "runtime_string", VALID, std=True)
    print("PASS String ABI: real String ordering/generics/aliases/references/format/parse native/source-removed QZI/COFF", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-string-abi-") as directory:
        check(args.qz.resolve(), Path(directory), args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
