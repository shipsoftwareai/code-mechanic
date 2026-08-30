package tool

func goComplex[T ~int](value T) T {
	return value + 1
}

func useGoComplex() int {
	return goComplex(4)
}
