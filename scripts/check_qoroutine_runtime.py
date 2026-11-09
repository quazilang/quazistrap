#!/usr/bin/env python3
"""Check the D-019 runtime groundwork end to end: an M:N scheduler prototype
written in Quazi (quazi.runtime.state anchor, spin-locked run queue, per-task
stacks entered with quazi.context.switch, four libc-free worker threads,
generic result slots) runs 1000 tasks natively and under `qz run --jit`.
This is the reference for the planned prelude scheduler behind `qo`."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;
import std.core;

@intrinsic("quazi.runtime.state")
unsafe fn runtime_state() *u8;
@intrinsic("quazi.context.switch")
unsafe fn context_switch(save: *u8, restore: *u8) void;
@intrinsic("quazi.thread.spawn_arg")
unsafe fn spawn_thread(f: WorkerEntry, arg: *u8) usize;
@syscall(24)
fn sched_yield() i64;

fn os_yield() void { unsafe { sched_yield(); } }

@repr(C)
type WorkerEntry = fn(*u8) *u8;
@repr(C)
type TaskEntry = fn(*u8) void;

// Scheduler state (runtime_state): +0 lock, +8 queue head, +16 queue tail, +24 started.
// Task (512 bytes): +0 state (2 = joinable), +8 result, +16 fn value, +24 stack,
// +32 next, +40 worker context, +48 runner, +56 finished, +64 task context (272 bytes).

unsafe fn lock(state: *u8) void {
    for (core.atomic_cas(state as *i64, 0, 1) != 0) { os_yield(); }
}

unsafe fn unlock(state: *u8) void {
    core.atomic_cas(state as *i64, 1, 0);
}

unsafe fn enqueue(task: *u8) void {
    const state = runtime_state();
    *((task + 32) as **u8) = 0;
    lock(state);
    const tail: *u8 = *((state + 16) as **u8);
    if (tail == 0) { *((state + 8) as **u8) = task; } else { *((tail + 32) as **u8) = task; }
    *((state + 16) as **u8) = task;
    unlock(state);
}

unsafe fn dequeue() *u8 {
    const state = runtime_state();
    lock(state);
    const head: *u8 = *((state + 8) as **u8);
    if (head != 0) {
        const next: *u8 = *((head + 32) as **u8);
        *((state + 8) as **u8) = next;
        if (next == 0) { *((state + 16) as **u8) = 0; }
    }
    unlock(state);
    head
}

@export("qo_proto_worker")
pub unsafe fn worker_main(_arg: *u8) *u8 {
    const context: *u8 = core.malloc(272);
    for (true) {
        const task = dequeue();
        if (task == 0) { os_yield(); continue; }
        *((task + 40) as **u8) = context;
        context_switch(context, task + 64);
        // Back on the worker stack: only now may a joiner free the task.
        if (*((task + 56) as *i64) == 1) { core.atomic_add(task as *i64, 2); } else { enqueue(task); }
    }
    ret 0;
}

@export("qo_proto_entry")
pub unsafe fn task_entry(task: *u8) void {
    const runner: fn(*u8) void = *((task + 48) as *fn(*u8) void);
    runner(task);
    // The worker publishes completion after this switch leaves the task stack.
    *((task + 56) as *i64) = 1;
    context_switch(task + 64, *((task + 40) as **u8));
}

unsafe fn run_task[T](task: *u8) void {
    const f: fn() T = *((task + 16) as *fn() T);
    const value: T = f();
    *((task + 8) as *T) = value;
}

unsafe fn start_workers() void {
    const state = runtime_state();
    if (core.atomic_cas((state + 24) as *i64, 0, 1) == 0) {
        for i : 0..4 { spawn_thread(worker_main, 0); }
    }
}

unsafe fn spawn_i64(f: fn() i64) *u8 {
    const task: *u8 = core.malloc(512);
    core.memset(task, 0, 512);
    *((task + 16) as *fn() i64) = f;
    const runner: fn(*u8) void = |t| run_task[i64](t);
    *((task + 48) as *fn(*u8) void) = runner;
    const stack: *u8 = core.malloc(65536);
    *((task + 24) as **u8) = stack;
    const entry: TaskEntry = task_entry;
    *((task + 64 + 48) as *TaskEntry) = entry;
    *((task + 64 + 56) as **u8) = stack + 65536 - 72;
    *((task + 64 + 72) as **u8) = task;
    *((task + 64 + 80) as *u32) = 8064;
    *((task + 64 + 84) as *u16) = 895;
    start_workers();
    enqueue(task);
    task
}

unsafe fn join_i64(task: *u8) i64 {
    for (core.atomic_add(task as *i64, 0) != 2) { os_yield(); }
    const value: i64 = *((task + 8) as *i64);
    core.free(*((task + 24) as **u8));
    core.free(task);
    value
}

fn square(n: i64) i64 { n * n }

fn main() i32 {
    var total: i64 = 0;
    unsafe {
        const tasks: *u8 = core.malloc(8 * 1000);
        for i : 0..1000 {
            const n: i64 = i as i64;
            const f: fn() i64 = || square(n);
            *((tasks + i * 8) as **u8) = spawn_i64(f);
        }
        for i : 0..1000 {
            total = total + join_i64(*((tasks + i * 8) as **u8));
        }
        core.free(tasks);
    }
    io.println("total {}", total);
    if (total != 332833500) { ret 1; }
    0
}
"""


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "scheduler"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="scheduler"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    for _ in range(5):
        assert "total 332833500" in invoke([str(binary)], case)
    assert "total 332833500" in invoke([str(qz), "run", "--jit", *common], case)
    print("PASS qoroutine runtime prototype: 1000 tasks on 4 libc-free workers (5 runs) and --jit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-qoroutine-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
