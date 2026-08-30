float glslEasy(float value) {
    return value + 1.0;
}

float glslComplex(float value);

float glslComplex(float value) {
    float total = 0.0;
    float weight = 0.0;

    for (int index = 0; index < 8; ++index) {
        float phase = value + float(index) * 0.17;
        vec3 sample = vec3(
            abs(sin(phase)),
            abs(cos(phase * 0.75)),
            fract(phase * 0.33)
        );
        float sampleWeight = float(index + 1) / 8.0;
        float luminance = dot(sample, vec3(0.2126, 0.7152, 0.0722));
        if (luminance < 0.05) {
            continue;
        }
        total += smoothstep(0.0, 1.0, luminance) * sampleWeight;
        weight += sampleWeight;
    }

    return weight == 0.0 ? value : clamp(total / weight, 0.0, 1.0);
}

void useGlslComplex() {
    float result = glslComplex(1.0);
}
