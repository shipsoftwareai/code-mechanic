@interface Renderer
- (int)frameCount;
- (void)renderFrame;
@end

@implementation Renderer
- (int)frameCount {
    return 1;
}

- (void)renderFrame {
    int samples[] = {3, -5, 8, 13, 21, 34};
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

    if (accepted > 0) {
        int average = total / accepted;
        (void)average;
    }
}

- (void)useRenderFrame {
    [self renderFrame];
}
@end
