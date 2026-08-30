#include <array>

int cppEasy(int value) {
    return value + 1;
}

int cppComplex(int value);

class Worker {
public:
    int run(int value) {
        return cppComplex(value);
    }
};

int cppComplex(int value) {
    const std::array<int, 7> samples = {
        value,
        value + 3,
        value - 2,
        value * 2,
        value + 7,
        value / 2,
        value + 11,
    };
    int total = 0;
    int accepted = 0;

    for (std::size_t index = 0; index < samples.size(); ++index) {
        const int candidate = samples[index] < 0 ? -samples[index] : samples[index];
        if ((candidate + static_cast<int>(index)) % 3 == 0) {
            continue;
        }

        total += candidate * static_cast<int>(index + 1);
        ++accepted;
    }

    return accepted == 0 ? value : total / accepted;
}
