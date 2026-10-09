#include <stdio.h>
#include <math.h>

long calc_add(long a, long b);
long calc_sub(long a, long b);
long calc_mul(long a, long b);
long calc_div(long a, long b);
double calc_sqrt(double x);
double calc_log(double x);

int main(void) {
    printf("dodawanie: 7 + 3 = %ld\n", calc_add(7, 3));
    printf("odejmowanie: 7 - 3 = %ld\n", calc_sub(7, 3));
    printf("mnozenie: 7 * 3 = %ld\n", calc_mul(7, 3));
    printf("dzielenie: 7 / 3 = %ld\n", calc_div(7, 3));
    printf("logarytm: log(2.718282) = %.6f\n", calc_log(2.718281828459045));
    printf("pierwiastek: sqrt(9) = %.6f\n", calc_sqrt(9.0));
    return 0;
}
