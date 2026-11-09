#!/usr/bin/env python3
"""Check `qo f(args)` (D-019): qoroutines on the prelude M:N scheduler run
natively, from source-removed QZI and under `qz run --jit`; Windows COFF and
NetBSD ELF objects emit; programs without `qo` do not contain the scheduler."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn square(n: i64) i64 { n * n }

fn is_even(n: i64) bool { n % 2 == 0 }

fn main() i32 {
    var total: i64 = 0;
    var evens: i64 = 0;
    const first = qo square(3 as i64);
    for i : 0..300 {
        const n: i64 = i as i64;
        const handle: Qoroutine[i64] = qo square(n);
        const even = qo is_even(n);
        total = total + handle.join().unwrap();
        if (even.join().unwrap()) { evens = evens + 1; }
    }
    total = total + first.join().unwrap();
    io.println("total {} evens {}", total, evens);
    if (total != 8955059 || evens != 150) { ret 1; }
    0
}
"""

EXPECTED = "total 8955059 evens 150"

# Owned arguments (String, structs) move into the qoroutine like call arguments.
OWNED = r"""import std.io;

pub struct Job { id: i64, label: String, }

fn measure(text: String) i64 { text.len() as i64 }

fn score(job: Job) i64 { job.id * 100 + job.label.len() as i64 }

fn main() i32 {
    var total: i64 = 0;
    for i : 0..500 {
        const text: String = "qoroutine".to_string();
        const job = Job { id: i as i64, label: "abc".to_string() };
        const a = qo measure(text);
        const b = qo score(job);
        total = total + a.join().unwrap() + b.join().unwrap();
    }
    io.println("owned {}", total);
    if (total != 500 * 9 + 100 * 124750 + 500 * 3) { ret 1; }
    0
}
"""

# Channel[T]: producers and a consumer in qoroutines, the main thread
# receiving, blocking on a small buffer, close and drain.
CHANNEL = r"""import std.io;

fn produce(ch: Channel[i64], from: i64, count: i64) i64 {
    for i : 0..count { ch.send(from + i as i64); }
    count
}

fn consume(ch: Channel[i64], done: Channel[i64]) i64 {
    var total: i64 = 0;
    for (true) {
        const value = ch.receive();
        if (value.is_none()) { break; }
        total = total + value.unwrap();
    }
    done.send(total);
    total
}

fn finish(ch: Channel[i64], producer: Qoroutine[i64]) i64 {
    const sent = producer.join().unwrap();
    ch.close();
    sent
}

fn main() i32 {
    const ch: Channel[i64] = Channel.new(8);
    const a = ch.clone();
    const b = ch.clone();
    const p1 = qo produce(a, 0 as i64, 1000 as i64);
    const p2 = qo produce(b, 1000 as i64, 1000 as i64);
    var total: i64 = 0;
    var received: i64 = 0;
    for (received < 2000) {
        const value = ch.receive();
        if (value.is_some()) {
            total = total + value.unwrap();
            received = received + 1;
        }
    }
    const sent = p1.join().unwrap() + p2.join().unwrap();

    const pipe: Channel[i64] = Channel.new(4);
    const done: Channel[i64] = Channel.new(1);
    const reader = pipe.clone();
    const reply = done.clone();
    const consumer = qo consume(reader, reply);
    const writer = pipe.clone();
    const producer = qo produce(writer, 1 as i64, 500 as i64);
    const closer = pipe.clone();
    const closed = qo finish(closer, producer);
    const piped = consumer.join().unwrap();
    const reported = done.receive().unwrap();
    closed.join().unwrap();
    io.println("channel {} {} {} {}", sent, total, piped, reported);
    if (total != 1999000 || piped != 125250 || reported != piped) { ret 1; }
    0
}
"""

# A panic inside a qoroutine is reported by join; the program continues.
PANIC = r"""import std.io;

fn risky(n: i64) i64 {
    if (n % 3 == 0) { panic("multiple of three"); }
    n
}

fn main() i32 {
    var ok: i64 = 0;
    var failed: i64 = 0;
    for i : 0..30 {
        const n: i64 = i as i64;
        const result = (qo risky(n)).join();
        if (result.is_ok()) { ok = ok + result.unwrap(); } else { failed = failed + 1; }
    }
    io.println("ok {} failed {}", ok, failed);
    if (failed != 10) { ret 1; }
    panic("main panics");
}
"""

# Eight qoroutines spin without ever yielding until all eight have started:
# with four workers only timer preemption lets the last four run (Linux).
SPIN = r"""import std.io;
import std.core;

unsafe fn spin(counter: *u8, total: i64) i64 {
    core.atomic_add(counter as *i64, 1);
    var loops: i64 = 0;
    for (core.atomic_add(counter as *i64, 0) < total) { loops = loops + 1; }
    1
}

fn main() i32 {
    var done: i64 = 0;
    unsafe {
        const counter: *u8 = core.malloc(8);
        *(counter as *i64) = 0;
        const a = qo spin(counter, 8 as i64);
        const b = qo spin(counter, 8 as i64);
        const c = qo spin(counter, 8 as i64);
        const d = qo spin(counter, 8 as i64);
        const e = qo spin(counter, 8 as i64);
        const f = qo spin(counter, 8 as i64);
        const g = qo spin(counter, 8 as i64);
        const h = qo spin(counter, 8 as i64);
        done = a.join().unwrap() + b.join().unwrap() + c.join().unwrap() + d.join().unwrap()
            + e.join().unwrap() + f.join().unwrap() + g.join().unwrap() + h.join().unwrap();
    }
    io.println("preempted {}", done);
    0
}
"""

# Preemption signals must not cut sleeps short.
SLEEP = r"""import std.io;
import std.core;
import std.time.Instant;

fn nap(ms: u64) u64 {
    const start = Instant.now().unwrap();
    core.sleep_ms(ms);
    const took = start.elapsed().unwrap();
    took.seconds() * 1000 + (took.subsec_nanoseconds() / 1000000) as u64
}

fn main() i32 {
    const slept = (qo nap(200 as u64)).join().unwrap();
    io.println("slept {}", slept);
    if (slept < 190) { ret 1; }
    0
}
"""

# Owned results (String, structs) move out through join, also next to panics.
RESULTS = r"""import std.io;

pub struct Pair { a: i64, b: String, }

fn label(n: i64) String {
    const text: String = "item-".to_string();
    text.push_str(n.to_string().as_str())
}

fn pair(n: i64) Pair { Pair { a: n, b: label(n) } }

fn main() i32 {
    var total: i64 = 0;
    for i : 0..200 {
        const n: i64 = i as i64;
        const text = (qo label(n)).join().unwrap();
        const p = (qo pair(n)).join().unwrap();
        total = total + text.len() as i64 + p.a + p.b.len() as i64;
    }
    io.println("total {}", total);
    0
}
"""

RESULT_PANICS = r"""import std.io;

fn label(n: i64) String {
    if (n % 2 == 0) { panic("even"); }
    const text: String = "odd-".to_string();
    text.push_str(n.to_string().as_str())
}

fn main() i32 {
    var ok: i64 = 0;
    var failed: i64 = 0;
    for i : 0..40 {
        const n: i64 = i as i64;
        const result = (qo label(n)).join();
        if (result.is_ok()) { ok = ok + result.unwrap().len() as i64; } else { failed = failed + 1; }
    }
    io.println("ok {} failed {}", ok, failed);
    0
}
"""

# Joins nested ten times deeper than there are workers: a waiting qoroutine
# must yield its worker instead of blocking it.
NESTED = r"""import std.io;

fn chain(depth: i64) i64 {
    if (depth == 0) { ret 0; }
    const child = qo chain(depth - 1);
    child.join().unwrap() + depth
}

fn main() i32 {
    var total: i64 = 0;
    for i : 0..20 {
        const top = qo chain(40 as i64);
        total = total + top.join().unwrap();
    }
    io.println("nested {}", total);
    if (total != 16400) { ret 1; }
    0
}
"""


def invoke(command, root, ok=True):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    if ok:
        assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "qoroutines"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="qoroutines"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    for _ in range(5):
        assert EXPECTED in invoke([str(binary)], case).stdout
    assert EXPECTED in invoke([str(qz), "run", "--jit", *common], case).stdout
    coff = case / "program.obj"
    invoke([str(qz), "build", *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], case)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    netbsd = case / "program.o"
    invoke([str(qz), "build", *common, "-c", "--target", "x86_64-netbsd", "-o", str(netbsd)], case)
    assert netbsd.read_bytes()[:4] == b"\x7fELF"
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    assert b"__qo_worker" in artifact.read_bytes()
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    assert EXPECTED in invoke([str(portable)], case).stdout

    nested = root / "nested"
    (nested / "src").mkdir(parents=True)
    (nested / "quazi.toml").write_text('[package]\nname="nested"\n[build]\nentry="src/main.qz"\n')
    (nested / "src/main.qz").write_text(NESTED)
    binary = nested / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], nested)
    for _ in range(3):
        assert "nested 16400" in subprocess.run([str(binary)], cwd=nested, text=True,
                                                stdout=subprocess.PIPE, timeout=60).stdout

    owned = root / "owned"
    (owned / "src").mkdir(parents=True)
    (owned / "quazi.toml").write_text('[package]\nname="owned"\n[build]\nentry="src/main.qz"\n')
    (owned / "src/main.qz").write_text(OWNED)
    binary = owned / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], owned)
    for _ in range(3):
        assert "owned 12481000" in invoke([str(binary)], owned).stdout
    (owned / "src/main.qz").write_text(
        "fn measure(text: String) i64 { text.len() as i64 }\n"
        "fn main() i32 {\n    const text: String = \"x\".to_string();\n"
        "    const a = qo measure(text);\n    const n = text.len();\n    a.join().unwrap();\n    0\n}\n")
    result = invoke([str(qz), "check"], owned, ok=False)
    assert result.returncode != 0 and "use of moved value 'text'" in result.stdout, result.stdout

    channel = root / "channel"
    (channel / "src").mkdir(parents=True)
    (channel / "quazi.toml").write_text('[package]\nname="channel"\n[build]\nentry="src/main.qz"\n')
    (channel / "src/main.qz").write_text(CHANNEL)
    binary = channel / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], channel)
    for _ in range(5):
        assert "channel 2000 1999000 125250 125250" in subprocess.run(
            [str(binary)], cwd=channel, text=True, stdout=subprocess.PIPE, timeout=60).stdout
    assert "channel 2000" in invoke([str(qz), "run", "--jit", *common], channel).stdout

    panicking = root / "panicking"
    (panicking / "src").mkdir(parents=True)
    (panicking / "quazi.toml").write_text('[package]\nname="panicking"\n[build]\nentry="src/main.qz"\n')
    (panicking / "src/main.qz").write_text(PANIC)
    binary = panicking / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], panicking)
    result = subprocess.run([str(binary)], cwd=panicking, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=60)
    assert "ok 300 failed 10" in result.stdout, (result.stdout, result.stderr)
    assert result.stderr.count("qoroutine panicked at 'multiple of three'") == 10, result.stderr
    # A panic outside qoroutines still ends the program with status 101.
    assert "thread 'main' panicked at 'main panics'" in result.stderr and result.returncode == 101

    spin = root / "spin"
    (spin / "src").mkdir(parents=True)
    (spin / "quazi.toml").write_text('[package]\nname="spin"\n[build]\nentry="src/main.qz"\n')
    (spin / "src/main.qz").write_text(SPIN)
    binary = spin / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], spin)
    for _ in range(5):
        assert "preempted 8" in subprocess.run([str(binary)], cwd=spin, text=True,
                                               stdout=subprocess.PIPE, timeout=60).stdout
    assert "preempted 8" in invoke([str(qz), "run", "--jit", *common], spin).stdout

    sleeper = root / "sleeper"
    (sleeper / "src").mkdir(parents=True)
    (sleeper / "quazi.toml").write_text('[package]\nname="sleeper"\n[build]\nentry="src/main.qz"\n')
    (sleeper / "src/main.qz").write_text(SLEEP)
    assert "slept" in invoke([str(qz), "run", *common], sleeper).stdout

    # With libc (pthread workers) preemption is off; qo still works by yielding.
    libc = root / "libc"
    (libc / "src").mkdir(parents=True)
    (libc / "quazi.toml").write_text(
        '[package]\nname="libc_qo"\n[build]\nentry="src/main.qz"\n'
        '[target.x86_64-linux.link]\nlibc = true\n')
    (libc / "src/main.qz").write_text(NESTED)
    binary = libc / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], libc)
    assert b"pthread_create" in binary.read_bytes()
    assert "nested 16400" in subprocess.run([str(binary)], cwd=libc, text=True,
                                            stdout=subprocess.PIPE, timeout=60).stdout

    results = root / "results"
    (results / "src").mkdir(parents=True)
    (results / "quazi.toml").write_text('[package]\nname="results"\n[build]\nentry="src/main.qz"\n')
    (results / "src/main.qz").write_text(RESULTS)
    assert "total 22880" in invoke([str(qz), "run", *common], results).stdout
    (results / "src/main.qz").write_text(RESULT_PANICS)
    binary = results / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], results)
    assert "ok 115 failed 20" in subprocess.run([str(binary)], cwd=results, text=True,
                                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                                timeout=60).stdout

    plain = root / "plain"
    (plain / "src").mkdir(parents=True)
    (plain / "quazi.toml").write_text('[package]\nname="plain"\n[build]\nentry="src/main.qz"\n')
    (plain / "src/main.qz").write_text('import std.io;\n\nfn main() i32 {\n    io.println("hi");\n    0\n}\n')
    artifact = plain / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], plain)
    # Destructor hooks (Channel.free) are always roots; the scheduler is not.
    for symbol in (b"__qo_worker", b"__qo_spawn", b"__qo_entry", b"__qo_init"):
        assert symbol not in artifact.read_bytes(), f"{symbol} must be dropped when unused"

    bad = root / "bad"
    (bad / "src").mkdir(parents=True)
    (bad / "quazi.toml").write_text('[package]\nname="bad"\n[build]\nentry="src/main.qz"\n')
    (bad / "src/main.qz").write_text("fn main() i32 {\n    const x = qo 5;\n    0\n}\n")
    result = invoke([str(qz), "check"], bad, ok=False)
    assert result.returncode != 0 and "`qo` must be followed by a function call" in result.stdout, result.stdout
    print("PASS qoroutines: qo on 4 workers (5 runs), 40-deep nested joins, owned String/struct args and results moved, Channel send/receive/close, panics reported by join, timer preemption of 8 spinners, full sleeps, libc mode, --jit, source-removed QZI, COFF/NetBSD emit, unused scheduler dropped, non-call rejected")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-qo-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
