float glslComplex(float value);

float glslComplex(float value) {
    return sin(value);
}

void useGlslComplex() {
    float result = glslComplex(1.0);
}
