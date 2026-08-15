#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

extern void* my_thread_fwd(void*);

__asm__(
    ".global my_thread_fwd\n"
    "my_thread_fwd:\n"
    "push %rbp\n"
    "mov %rsp, %rbp\n"
    "sub $0x10, %rsp\n"
    "mov %rdi, -0x8(%rbp)\n"
    "mov %rsi, -0x10(%rbp)\n"
    "mov -0x10(%rbp), %rdi\n"
    "call thread_work_c\n"
    "mov %rax, -0x8(%rbp)\n"
    "mov -0x8(%rbp), %rax\n"
    "mov %rbp, %rsp\n"
    "pop %rbp\n"
    "ret\n"
);

void *thread_work_c(void *arg) {
    printf("Inside thread_work_c!\n");
    return NULL;
}

int main() {
    pthread_t t;
    void *env_ptr = malloc(16);
    int ret = pthread_create(&t, NULL, my_thread_fwd, env_ptr);
    printf("pthread_create returned: %d\n", ret);
    if (ret == 0) {
        pthread_join(t, NULL);
        printf("Joined successfully\n");
    }
    return 0;
}
