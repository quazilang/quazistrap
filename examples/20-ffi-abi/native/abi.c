#include <stdint.h>

typedef struct Point {
    double x;
    double y;
} Point;

typedef struct Sample {
    float value;
    int32_t tag;
} Sample;

typedef struct Triple {
    int64_t a;
    int64_t b;
    int64_t c;
} Triple;

extern Point quazi_scale_point(Point point, double factor);
extern Sample quazi_adjust_sample(Sample sample);
extern Triple quazi_bump_triple(Triple triple);
extern int64_t quazi_sum8(
    int64_t a, int64_t b, int64_t c, int64_t d,
    int64_t e, int64_t f, int64_t g, int64_t h
);

Point c_roundtrip_point(Point point, float bias) {
    point.x += bias;
    return quazi_scale_point(point, 2.0);
}

Sample c_roundtrip_sample(Sample sample) {
    sample.value += 0.25f;
    sample.tag += 1;
    return quazi_adjust_sample(sample);
}

Triple c_roundtrip_triple(Triple triple) {
    triple.a += 1;
    triple.b += 1;
    triple.c += 1;
    return quazi_bump_triple(triple);
}

int64_t c_sum8(
    int64_t a, int64_t b, int64_t c, int64_t d,
    int64_t e, int64_t f, int64_t g, int64_t h
) {
    return a + b + c + d + e + f + g + h;
}

int64_t c_call_quazi_sum8(void) {
    return quazi_sum8(1, 2, 3, 4, 5, 6, 7, 8);
}
