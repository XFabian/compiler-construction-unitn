// "Undefined on a Path": x has no definition when p is 0.
int f(int p) {
    int x;
    if (p) {
        x = 1;
    }
    return x;
}
