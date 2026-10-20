// The running example of the lecture, in C. Paste into godbolt.org.
long f(long x) {
    long a = x + 1;
    long b = x * 2;
    long c = b + 1;
    long d = c * 2;
    long e = b + d;
    return a + e;
}
