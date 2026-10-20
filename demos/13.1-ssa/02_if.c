// "Branches: Which x?": two definitions meet after the if.
int f(int p) {
    int x;
    if (p) {
        x = 1;
    } else {
        x = 2;
    }
    return x;
}
