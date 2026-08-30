int cppComplex(int value);

class Worker {
public:
    int run(int value) {
        return cppComplex(value);
    }
};

int cppComplex(int value) {
    return value + 1;
}
