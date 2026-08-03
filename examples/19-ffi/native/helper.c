extern int quazi_multiply(int left, int right);

int c_add(int left, int right) {
    return left + right;
}

int c_roundtrip(int left, int right) {
    return quazi_multiply(c_add(left, right), 2);
}
