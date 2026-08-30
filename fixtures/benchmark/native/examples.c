/* Context 01 */
/* Context 02 */
/* Context 03 */
/* Context 04 */
/* Context 05 */
/* Context 06 */
/* Context 07 */
/* Context 08 */
/* Context 09 */
/* Context 10 */
/* Context 11 */
/* Context 12 */
/* Context 13 */
/* Context 14 */
/* Context 15 */
/* Context 16 */
/* Context 17 */
/* Context 18 */
/* Context 19 */
/* Context 20 */
/* Context 21 */
/* Context 22 */
/* Context 23 */
/* Context 24 */

int c_easy(int value) {
    return value + 1;
}

int use_c_easy(void) {
    return c_easy(3);
}

int c_complex(int value);

int
c_complex(
    int value
) {
    int samples[] = {value, value + 3, value - 2, value * 2, value + 7, value / 2};
    int total = 0;
    int accepted = 0;

    for (int index = 0; index < 6; ++index) {
        int candidate = samples[index];
        if (candidate < 0) {
            candidate = -candidate;
        }
        if ((candidate + index) % 3 == 0) {
            continue;
        }

        total += candidate * (index + 1);
        accepted += 1;
    }

    if (accepted == 0) {
        return value;
    }
    return total / accepted;
}

int use_c_complex(void) {
    return c_complex(4);
}

struct CallbackTable {
    int (*invoke)(int);
};

int use_callback(struct CallbackTable table) {
    return table.invoke(3);
}

/* Trailing context 01 */
/* Trailing context 02 */
/* Trailing context 03 */
/* Trailing context 04 */
/* Trailing context 05 */
/* Trailing context 06 */
/* Trailing context 07 */
/* Trailing context 08 */
/* Trailing context 09 */
/* Trailing context 10 */
/* Trailing context 11 */
/* Trailing context 12 */
/* Trailing context 13 */
/* Trailing context 14 */
/* Trailing context 15 */
/* Trailing context 16 */
/* Trailing context 17 */
/* Trailing context 18 */
/* Trailing context 19 */
/* Trailing context 20 */
/* Trailing context 21 */
/* Trailing context 22 */
/* Trailing context 23 */
/* Trailing context 24 */
