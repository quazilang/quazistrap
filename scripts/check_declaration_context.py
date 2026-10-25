#!/usr/bin/env python3
"""Check defining-source signatures and alias binders across artifact paths."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from check_numeric_mutations import project, run

COMMON = ["--no-incremental", "--silent", "--no-color", "--no-unicode"]
REAL = "pub type Word=i32; pub struct Record{value:i32,}"
HELPER = """import ./real as own;
import ./real.Word as T;
pub fn roundtrip(value:own.Word)own.Word{ret value;}
pub fn collision[Word](value:own.Word)own.Word{ret value;}
type Pick[Record]=own.Record;
type Concrete[Word]=own.Word;
type Identity[Word]=Word;
pub fn alias_value(value:Concrete[u64])Concrete[u64]{ret value;}
"""
INVALID = """pub struct Bad{}
pub trait Wrong{fn accept[T](self:&Self,value:own.Word)own.Word;}
impl Wrong for Bad{fn accept[T](self:&Bad,value:T)own.Word{ret 0;}}
"""


def fixture(root, helper, main):
    project(root, main)
    (root / "real.qz").write_text(REAL)
    (root / "helper.qz").write_text(helper)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    qz = args.qz.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="quazi-declaration-context-") as directory:
        root = Path(directory)
        invalid = root / "invalid"
        fixture(invalid, HELPER + INVALID, "import ./helper; fn main()void{}")
        if args.before_qz:
            # Check only: the previous compiler accepts the mismatched trait.
            run([str(args.before_qz.resolve(strict=True)), "check"], invalid)
        for options in [["check"], ["build", *COMMON], ["build", *COMMON, "-i"],
                        ["build", *COMMON, "-c", "--target", "x86_64-windows"]]:
            result = subprocess.run([str(qz), *options], cwd=invalid, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            assert result.returncode != 0, (options, result.stdout)
            assert "S14" in result.stdout and "parameter 2 must" in result.stdout, result.stdout
            assert "got `T`" in result.stdout and "$alias" not in result.stdout, result.stdout
        print("PASS declaration mismatch rejected by check/native/QZI/COFF", flush=True)

        valid = root / "valid"
        fixture(valid, HELPER,
                "import ./helper; import ./helper.roundtrip as invoke; fn main()i32{if invoke(7)!=7{ret 1;}if helper.collision[u64](8)!=8{ret 2;}if helper.alias_value(9)!=9{ret 3;}ret 0;}")
        run([str(qz), "check"], valid)
        native = valid / "native"
        run([str(qz), "build", *COMMON, "-o", str(native)], valid)
        run([str(native)], valid)
        artifact = valid / "program.qzi"
        run([str(qz), "build", *COMMON, "-i", "-o", str(artifact)], valid)
        for source in valid.glob("*.qz"):
            source.unlink()
        emission = root / "emission"
        emission.mkdir()
        native = emission / "native"
        run([str(qz), "build", str(artifact), *COMMON, "-o", str(native)], emission)
        run([str(native)], emission)
        coff = emission / "program.obj"
        run([str(qz), "build", str(artifact), *COMMON, "-c", "--target", "x86_64-windows",
             "-o", str(coff)], emission)
        assert coff.read_bytes()[:2] == b"\x64\x86"
        print("PASS defining-source signatures native/source-removed QZI/Windows COFF", flush=True)


if __name__ == "__main__":
    main()
