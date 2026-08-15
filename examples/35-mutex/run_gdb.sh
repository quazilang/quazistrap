#!/bin/bash
gdb -batch -ex "set pagination off" -ex "b pthread_create" -ex "run" -ex "info registers" -ex "x/10g \$rdi" -ex "x/10g \$rdx" -ex "x/10g \$rcx" build/mutex
