// "One Name, Many Values": the straight-line example. Paste into godbolt.org.
int f(void) {
    int a = 1;
    int b = a + 2;
    a = 7;
    return a + b;
}
