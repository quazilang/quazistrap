#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

void *thread_work(void *arg) {
    return NULL;
}

int main() {
    pthread_t *t = malloc(sizeof(pthread_t));
    int ret = pthread_create(t, NULL, thread_work, NULL);
    printf("pthread_create returned: %d\n", ret);
    if (ret == 0) {
        printf("pthread_t value: %p\n", (void *)*t);
        pthread_join(*t, NULL);
        printf("Joined successfully\n");
    }
    return 0;
}
