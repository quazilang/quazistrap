#!/usr/bin/env python3
"""Verify source initialization paths, match execution and old-cache rejection."""

import argparse
import subprocess
import tempfile
from pathlib import Path


INVALID = {
    "partial_if": ("fn test(flag:bool)i32 { var value:i32; if flag {value=1;} ret value; }", "S02"),
    "sibling_if": ("fn test(flag:bool)i32 { var value:i32; if flag {value=1;} else {ret value;} ret 0; }", "S02"),
    "else_if": ("fn test(flag:bool)i32 { var value:i32; if flag {value=1;} else if value>0 {ret 1;} ret 0; }", "S02"),
    "empty_loop": ("fn test(flag:bool)i32 { var value:i32; for flag {value=1;break;} ret value; }", "S02"),
    "first_body": ("fn test()i32 { var value:i32; for var index=0; index<1; value=1 {ret value;} ret 0; }", "S02"),
    "continue_update": ("fn test(flag:bool)i32 { var value:i32; for var index=0; index<1; index=value {if flag {continue;} value=1;} ret 0; }", "S02"),
    "partial_break": ("fn test(flag:bool)i32 { var value:i32; for {if flag {break;} value=1;break;} ret value; }", "S02"),
    "empty_range": ("fn test()i32 { var value:i32; for index:0..0 {value=1;} ret value; }", "S02"),
    "and_skip": ("fn test(flag:bool)bool { var value:bool; flag && (value=true); ret value; }", "S02"),
    "or_skip": ("fn test(flag:bool)bool { var value:bool; flag || (value=true); ret value; }", "S02"),
    "constant_and_skip": ("fn test()bool { var value:bool; false && (value=true); ret value; }", "S02"),
    "constant_or_skip": ("fn test()bool { var value:bool; true || (value=true); ret value; }", "S02"),
    "partial_match": ("fn test(flag:bool)i32 { var value:i32; match flag {true=>value=1,false=>0,}; ret value; }", "S02"),
    "sibling_match": ("fn test(flag:bool)i32 { var value:i32; ret match flag {true=>value=1,false=>value,}; }", "S02"),
    "guard_mismatch": ("fn test(flag:bool)i32 { var value:i32; ret match flag {true if (value=1)>0=>0,false=>value,_=>0,}; }", "S02"),
    "pattern_shadow": ("fn test(flag:bool)i32 { var value:i32; match flag {value=>0,}; ret value; }", "S02"),
    "callee": ("fn test()i32 { var action:fn()i32; ret action(); }", "S02"),
    "callback": ("@repr(C) type Callback=fn()i32; unsafe fn test()i32 { var action:Callback; ret action(); }", "S02"),
    "deferred_assignment": ("fn test()i32 { var value:i32; var action=|| value=1; ret value; }", "S02"),
    "scalar_match": ("fn test(value:i32)i32 { ret match value {1=>1,}; }", "S09"),
    "bool_match": ("fn test(value:bool)i32 { ret match value {true=>1,}; }", "S09"),
    "float_match": ("fn test(value:f64)i32 { ret match value {1.0=>1,}; }", "S09"),
    "string_match": ('fn test(value:str)i32 { ret match value {"one"=>1,}; }', "S09"),
    "nul_match": (r'fn test()i32 {var value:i32;match "a\0b" {"a"=>0,_=>value=7,};ret value;}', "S02"),
    "string_pattern_int": ('fn test()i32 {ret match 1 {"one"=>0,_=>1,};}', "S01"),
    "float_pattern_int": ('fn test()i32 {ret match 0 {0.0=>0,_=>1,};}', "S01"),
    "named_call": ("fn select(first:i32,second:i32)i32{ret first;}fn test()i32{var value:i32;ret select(second=(value=7),first=value+1);}", "S02"),
    "named_method": ("struct Reader{} impl Reader{fn select(self:&Reader,first:i32,second:i32)i32{ret first;}}fn test()i32{var reader=Reader{};var value:i32;ret reader.select(second=(value=7),first=value+1);}", "S02"),
    "named_static": ("struct Reader{} impl Reader{fn select(first:i32,second:i32)i32{ret first;}}fn test()i32{var value:i32;ret Reader.select(second=(value=7),first=value+1);}", "S02"),
    "named_module": ("import choices;fn test()i32{var value:i32;ret choices.select(second=(value=7),first=value+1);}", "S02"),
    "bool_integer_pattern": ("fn test(flag:bool)i32{var value:i32;ret match flag{1=>0,0=>value=7,_=>value,};}", "S02"),
    "nested_literal": ('enum Wrap{Some(i32),None,}fn test()i32{var value:Wrap=Wrap.Some(1);ret match value{Some("one")=>0,_=>1,};}', "S01"),
    "untyped_nested_literal": ('enum Wrap{Some(i32),None,}fn test()i32{var value=Wrap.Some(1);ret match value{Some("one")=>0,_=>1,};}', "S14"),
    "generic_nested_literal": ('enum Wrap[T]{Some(T),None,}fn test()i32{var value:Wrap[i32]=Wrap.Some(1);ret match value{Some("one")=>0,_=>1,};}', "S01"),
    "ambiguous_untyped_pattern": ("enum First{Some(i32),None,}fn test()i32{var value=First.Some(1);ret match value{Some(_)=>7,_=>0,};}", "S14"),
    "struct_field_schedule": ("struct Pair{first:i32,second:i32,}fn test()i32{var value:i32;var pair=Pair{second:(value=7),first:value+1};ret pair.first;}", "S02"),
    "indirect_callee_schedule": ("fn helper(value:i32)i32{ret value;}fn get(value:i32)fn(i32)i32{ret helper;}fn test()i32{var value:i32;ret get(value)((value=7));}", "S02"),
    "duplicate_struct_field": ("struct Pair{first:i32,second:i32,}fn test()i32{var value:i32;var pair=Pair{first:0,second:0,second:(value=7)};ret value;}", "S07"),
    "float_absorber": ("fn test(input:f64)i32{var value:i32;match 0.0*input{0.0=>value=7,_=>0,};ret value;}", "S02"),
}

# Each check calls a separate function to keep the QZI register budget local.
VALID = {
    "both_if": ("fn both_if(flag:bool)i32 {var value:i32;if flag {value=1;}else{value=2;}ret value;}", "both_if(true)==1 && both_if(false)==2"),
    "return_if": ("fn return_if(flag:bool)i32 {var value:i32;if flag {unsafe{ret 0;}}else{value=2;}ret value;}", "return_if(true)==0 && return_if(false)==2"),
    "and_truth": ("fn and_truth(flag:bool)i32 {var value:i32;if flag && (value=5) {ret value;}ret 0;}", "and_truth(true)==5 && and_truth(false)==0"),
    "or_false": ("fn or_false(flag:bool)i32 {var value:i32;if flag || (value=0) {ret 1;}else{ret value;}}", "or_false(true)==1 && or_false(false)==0"),
    "negation": ("fn negation(flag:bool)i32 {var value:i32;if !(flag || (value=0)) {ret value;}ret 1;}", "negation(true)==1 && negation(false)==0"),
    "bool_cast": ("fn bool_cast(flag:bool)i32 {var value:i32;if ((flag && (value=5)) as bool) {ret value;}ret 0;}", "bool_cast(true)==5 && bool_cast(false)==0"),
    "logical_run": ("fn logical_run()bool {var first:bool;var second:bool;true && (first=true);false || (second=true);ret first && second;}", "logical_run()"),
    "all_breaks": ("fn all_breaks(flag:bool)i32 {var value:i32;for{if flag{value=1;break;}value=2;break;}ret value;}", "all_breaks(true)==1 && all_breaks(false)==2"),
    "init_once": ("fn init_once()i32 {var value:i32;for value=5; false; {}ret value;}", "init_once()==5"),
    "condition_once": ("fn condition_once()i32 {var value:i32;for (value=0) {}ret value;}", "condition_once()==0"),
    "continue_input": ("fn continue_input()i32 {var value:i32;for var index=0;index<1;index=value {value=1;continue;}ret 0;}", "continue_input()==0"),
    "nested_loop": ("fn nested_loop()i32 {var value:i32;for{for{break;}value=3;unsafe{break;}value=2;}ret value;}", "nested_loop()==3"),
    "terminal_if": ("fn terminal_if()i32 {var flag:bool;if (flag=true){ret 3;}unknown_dead_function();}", "terminal_if()==3"),
    "terminal_loop": ("fn terminal_loop()i32 {for{ret 4;}unknown_dead_function();}", "terminal_loop()==4"),
    "terminal_unsafe": ("fn terminal_unsafe()i32 {unsafe{ret 5;}unknown_dead_function();}", "terminal_unsafe()==5"),
    "match_all": ("fn match_all(flag:bool)i32 {var value:i32;match flag{true=>value=1,false=>value=2,};ret value;}", "match_all(true)==1 && match_all(false)==2"),
    "match_scrutinee": ("fn match_scrutinee()i32 {var value:i32;match (value=5){5=>0,_=>1,};ret value;}", "match_scrutinee()==5"),
    "guard_body": ("fn guard_body(seed:i32)i32 {var value:i32;ret match seed{_ if(value=5)>0=>value,_=>0,};}", "guard_body(0)==5 && guard_body(1)==5"),
    "guard_next": ("fn guard_next(seed:i32)i32 {var value:i32;ret match seed{_ if(value=0)=>1,_=>value,};}", "guard_next(0)==0 && guard_next(1)==0"),
    "guard_true": ("fn guard_true(seed:i32)i32 {ret match 7{_ if true=>10,_=>20,};}", "guard_true(0)==10 && guard_true(1)==10"),
    "guard_false": ("fn guard_false(seed:i32)i32 {ret match 7{_ if false=>10,_=>20,};}", "guard_false(0)==20 && guard_false(1)==20"),
    "guard_first_param": ("fn guard_first_param(flag:bool)i32 {ret match 7{_ if flag=>10,_=>20,};}", "guard_first_param(true)==10 && guard_first_param(false)==20"),
    "sibling_shadow": ("fn sibling_shadow(flag:bool)i32 {var value:i32=7;ret match flag{value if false=>0,_=>value,};}", "sibling_shadow(true)==7 && sibling_shadow(false)==7"),
    "outer_shadow": ("fn outer_shadow(flag:bool)i32 {var value:i32=7;match flag{value=>0,};ret value;}", "outer_shadow(true)==7 && outer_shadow(false)==7"),
    "float_zero": ("fn float_zero(value:f64)i32 {ret match value{0.0=>10,_=>20,};}", "float_zero(-0.0)==10 && float_zero(0.0)==10 && float_zero(1.0)==20"),
    "float_constant": ("fn float_constant()i32 {ret match -0.0{0.0=>10,};}", "float_constant()==10"),
    "float_nan": ("fn divide(a:f64,b:f64)f64 {ret a/b;}fn float_nan(value:f64)i32 {ret match value{0.0=>1,_=>2,};}", "float_nan(divide(0.0,0.0))==2"),
    "void_match": ("fn ignore()void {}fn void_match(value:i32)i32 {match value{1=>ignore(),};ret 7;}", "void_match(1)==7 && void_match(2)==7"),
    "nul_prefix": (r'fn nul_prefix()i32 {var value:i32;match "a\0b" {"a"=>value=7,_=>0,};ret value;}', "nul_prefix()==7"),
    "named_call": ("fn select(first:i32,second:i32)i32{ret second;}fn named_call()i32{var value:i32;ret select(second=value+1,first=(value=7));}", "named_call()==8"),
    "named_method": ("struct Reader{} impl Reader{fn select(self:&Reader,first:i32,second:i32)i32{ret second;}}fn named_method()i32{var reader=Reader{};var value:i32;ret reader.select(second=value+1,first=(value=7));}", "named_method()==8"),
    "named_static": ("struct StaticReader{} impl StaticReader{fn select(first:i32,second:i32)i32{ret second;}}fn named_static()i32{var value:i32;ret StaticReader.select(second=value+1,first=(value=7));}", "named_static()==8"),
    "named_module": ("import choices;fn named_module()i32{var value:i32;ret choices.select(second=value+1,first=(value=7));}", "named_module()==8"),
    "bool_noncanonical": ("fn bool_noncanonical(flag:bool)i32{var value:i32;match flag{true=>value=7,false=>value=9,};ret value;}", "bool_noncanonical(2)==7 && bool_noncanonical(0)==9 && bool_noncanonical(1)==7"),
    "bool_integer_exact": ("fn bool_integer_exact(flag:bool)i32{ret match flag{1=>7,0=>9,_=>11,};}", "bool_integer_exact(2)==11 && bool_integer_exact(0)==9 && bool_integer_exact(1)==7"),
    "bool_int_truth": ("fn bool_int_truth(value:i32)i32{ret match value{true=>7,false=>9,};}", "bool_int_truth(2)==7 && bool_int_truth(0)==9"),
    "bool_const_truth": ("fn bool_const_truth()i32{const flag:bool=2;var value:i32;match flag{true=>value=7,_=>0,};ret value;}", "bool_const_truth()==7"),
    "borrowed_int": ("fn borrowed_int(value:&i32)i32{ret match value{1=>7,_=>9,};}fn check_borrowed_int_value()bool{var value=1;ret borrowed_int(&value)==7;}", "check_borrowed_int_value()"),
    "borrowed_bool": ("fn borrowed_bool(value:&bool)i32{ret match value{true=>7,false=>9,};}fn check_borrowed_bool_value()bool{var value:bool=2;ret borrowed_bool(&value)==7;}", "check_borrowed_bool_value()"),
    "borrowed_float": ("fn borrowed_float(value:&f64)i32{ret match value{0.0=>7,_=>9,};}fn check_borrowed_float_value()bool{var value=-0.0;ret borrowed_float(&value)==7;}", "check_borrowed_float_value()"),
    "borrowed_string": ('fn borrowed_string(value:&str)i32{ret match value{"one"=>7,_=>9,};}fn check_borrowed_string_value()bool{var value="one";ret borrowed_string(&value)==7;}', "check_borrowed_string_value()"),
    "nested_typed": ("enum NestedValue[T]{Some(T),None,}fn nested_typed()i32{var value:NestedValue[i32]=NestedValue.Some(1);ret match value{Some(1)=>7,_=>9,};}", "nested_typed()==7"),
    "generic_payload_binding": ("enum PairValue[T]{Both(i32,T),}fn generic_payload_binding()i32{var value:PairValue[i32]=PairValue.Both(1,7);ret match value{Both(_,second)=>second,};}", "generic_payload_binding()==7"),
    "colliding_variant_names": ("enum OtherChoice{None,Some(i32),}fn colliding_variant_names()i32{var first:NestedValue[i32]=NestedValue.Some(1);var second:OtherChoice=OtherChoice.Some(1);ret (match first{Some(1)=>7,_=>0,})+(match second{Some(1)=>9,_=>0,});}", "colliding_variant_names()==16"),
    "void_payload_miss": ("enum VoidChoice{Some(i32),None,}fn void_payload_miss()i32{var value:VoidChoice=VoidChoice.Some(2);match value{Some(1)=>ignore(),None=>ignore(),};ret 7;}", "void_payload_miss()==7"),
    "field_target_schedule": ("struct TargetHolder{field:i32,}fn make_target(value:i32)TargetHolder{ret TargetHolder{field:value};}fn field_target_schedule()i32{var value:i32;make_target((value=7)).field=value;ret value;}", "field_target_schedule()==7"),
    "deref_target_schedule": ("unsafe fn target_pointer(value:i32,slot:*i32)*i32{ret slot;}fn deref_target_schedule()i32{var value:i32;var slot=0;unsafe{*target_pointer((value=7),&slot)=value;}ret slot;}", "deref_target_schedule()==7"),
    "range_shadow": ("fn range_shadow()i32{var index=7;for index:index..8{ret index;}ret 0;}", "range_shadow()==7"),
    "range_end_shadow": ("fn range_end_shadow()i32{var index=8;for index:7..index{ret index;}ret 0;}", "range_end_shadow()==7"),
    "absorbing_effect": ("fn absorbing_effect()i32{var value:i32;(value=7)*0;ret value;}", "absorbing_effect()==7"),
    "absorbing_logical": ("fn absorbing_logical()i32{var value:i32;(value=7)||true;ret value;}", "absorbing_logical()==7"),
    "absorbing_if": ("fn absorbing_if()i32{var value:i32;if ((value=7)||true){ret value;}ret 0;}", "absorbing_if()==7"),
    "absorbing_loop": ("fn absorbing_loop()i32{var value:i32;for ((value=7)>0)&&false {}ret value;}", "absorbing_loop()==7"),
    "borrowed_mut_string": ('fn borrowed_mut_string(value:&str!)i32{ret match value{"one"=>7,_=>9,};}fn check_mut_string_value()bool{var value="one";ret borrowed_mut_string(&value!)==7;}', "check_mut_string_value()"),
    "float_absorbing_nan": ("fn float_absorbing_nan(value:f64)i32{ret match 0.0*value{0.0=>7,_=>9,};}", "float_absorbing_nan(divide(0.0,0.0))==9"),
    "float_integer_zero": ("fn float_integer_zero(value:f64)i32{ret match value*0{0.0=>7,_=>9,};}", "float_integer_zero(divide(0.0,0.0))==9"),
    "match_scrutinee_snapshot": ("fn match_scrutinee_snapshot(flag:bool)i32{var value:i32;ret match flag{false=>value=7,true if (flag=false)=>0,true=>value=7,_=>value,};}", "match_scrutinee_snapshot(true)==7 && match_scrutinee_snapshot(false)==7"),
}


def invoke(command, root, succeeds=True):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    assert (result.returncode == 0) == succeeds, f"{command!r} exited {result.returncode}:\n{result.stdout}"
    return result.stdout


def project(root, source):
    root.mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="initialization"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="main.qz"\n'
    )
    (root / "main.qz").write_text(source)
    (root / "choices.qz").write_text("pub fn select(first:i32,second:i32)i32{ret second;}")


def valid_source():
    functions = []
    checks = []
    for index, (name, (source, check)) in enumerate(VALID.items(), 1):
        functions.append(source + f"\nfn check_{name}()bool {{ ret {check}; }}")
        checks.append(f"if !check_{name}() {{ret {index};}}")
    return "\n".join(functions) + "\nfn main()i32 {" + "\n".join(checks) + "ret 0;}\n"


def check(qz, directory, before):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    for name, (source, diagnostic) in INVALID.items():
        root = directory / name
        project(root, source + "\nfn main()i32 {ret 0;}")
        output = invoke([str(qz), "check"], root, succeeds=False)
        assert f"error[{diagnostic}]" in output, output
        for mode, suffix in [([], "native"), (["-i"], "qzi"),
                             (["-c", "--target", "x86_64-windows"], "obj")]:
            artifact = root / f"rejected.{suffix}"
            output = invoke([str(qz), "build", *common, *mode, "-o", str(artifact)], root, succeeds=False)
            assert f"error[{diagnostic}]" in output and not artifact.exists(), output
    print(f"PASS initialization: {len(INVALID)} invalid sources rejected by check/native/QZI/COFF")

    if before:
        root = directory / "cache"
        project(root, INVALID["partial_if"][0] + "\nfn main()i32 {ret test(false);}")
        binary = root / "native"
        args = ["build", "--no-color", "--no-unicode", "-o", str(binary)]
        invoke([str(before), *args], root)
        binary.unlink()  # Keep the old cache, but never run an undefined read.
        output = invoke([str(qz), *args], root, succeeds=False)
        assert "cache lookup miss" in " ".join(output.split()) and "error[S02]" in output, output
        assert not binary.exists()
        (root / "main.qz").write_text(VALID["guard_true"][0] + "\nfn main()i32 {ret guard_true(1)-10;}")
        invoke([str(before), *args], root)
        old = subprocess.run([str(binary)], cwd=root, timeout=60, capture_output=True)
        assert old.returncode == 10, "baseline must reproduce guard comparison against parameter r0"
        output = invoke([str(qz), *args], root)
        assert "cache lookup miss" in " ".join(output.split()), output
        invoke([str(binary)], root)
        output = invoke([str(qz), *args], root)
        assert "cache lookup hit" in " ".join(output.split()), output
        invoke([str(binary)], root)
        print("PASS initialization cache: old invalid read rejected, guard fixed, exact hit executes")
        alias_root = directory / "alias-cache"
        project(alias_root, "unsafe fn write(slot:*i32)void{*slot=7;}fn main()i32{var slot=0;unsafe{write(&slot);}ret slot;}")
        alias_binary = alias_root / "native"
        alias_args = ["build", "--no-color", "--no-unicode", "-o", str(alias_binary)]
        invoke([str(before), *alias_args], alias_root)
        result = subprocess.run([str(alias_binary)], cwd=alias_root, capture_output=True, timeout=60)
        assert result.returncode == 0, "baseline must reproduce stale aliased constant"
        for expected_cache in ["miss", "hit"]:
            output = invoke([str(qz), *alias_args], alias_root)
            assert f"cache lookup {expected_cache}" in " ".join(output.split()), output
            result = subprocess.run([str(alias_binary)], cwd=alias_root, capture_output=True, timeout=60)
            assert result.returncode == 7, f"aliased store lost after {expected_cache}: {result.returncode}"
        print("PASS initialization cache: stale aliased constant fixed after old miss and new exact hit")


    root = directory / "valid"
    project(root, valid_source())
    invoke([str(qz), "check"], root)
    binary = root / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], root)
    invoke([str(binary)], root)
    artifact = root / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], root)
    (root / "main.qz").unlink()
    emission = directory / "artifact-only"
    emission.mkdir()
    portable = emission / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], emission)
    invoke([str(portable)], emission)
    coff = emission / "program.obj"
    invoke([str(qz), "build", str(artifact), *common, "-c", "--target",
            "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print(f"PASS initialization: {len(VALID)} checks, native/source-removed QZI/Windows COFF")

    root = directory / "unmatched-value"
    project(root, "enum Value{Some(i32),None,}fn main()i32{var value:Value=Value.Some(2);ret match value{Some(1)=>7,None=>0,};}")
    binary = root / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], root)
    result = subprocess.run([str(binary)], cwd=root, capture_output=True, timeout=60)
    assert result.returncode == -4, f"unmatched value must trap: {result.returncode}"
    artifact = root / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], root)
    (root / "main.qz").unlink()
    binary = emission / "unmatched"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(binary)], emission)
    result = subprocess.run([str(binary)], cwd=emission, capture_output=True, timeout=60)
    assert result.returncode == -4, f"QZI unmatched value must trap: {result.returncode}"
    print("PASS initialization: unmatched value traps in native/source-removed QZI; Void preserves fallthrough")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-initialization-") as directory:
        check(args.qz.resolve(), Path(directory),
              args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
