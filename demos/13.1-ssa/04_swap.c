// Trap 2: after copy propagation, the swap lives in the φs.
int swap(int n) {
    int a = 1;
    int b = 2;
    while (n > 0) {
        int t = a;
        a = b;
        b = t;
        n = n - 1;
    }
    return a;
}
