package tool

func goEasy(value int) int {
	return value + 1
}

func useGoEasy() int {
	return goEasy(4)
}

func goComplex[T ~int](values []T, limit int) (T, int) {
	if limit < 0 {
		limit = 0
	}
	if limit > len(values) {
		limit = len(values)
	}

	var total T
	accepted := 0
	for index, value := range values[:limit] {
		if value < 0 {
			value = -value
		}
		if (int(value)+index)%3 == 0 {
			continue
		}

		total += value * T(index+1)
		accepted++
	}
	return total, accepted
}

func useGoComplex() int {
	total, accepted := goComplex([]int{3, -5, 8, 13, 21}, 4)
	if accepted == 0 {
		return 0
	}
	return total / accepted
}
