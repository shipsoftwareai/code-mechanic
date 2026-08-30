package dev.codemechanic.fixtures

data class WorkItem(
    val id: String,
    val priority: Int,
    val enabled: Boolean = true,
)

interface WorkEmitter {
    fun emit(message: String)
}

fun kotlinPadBefore00(): Int = 0
fun kotlinPadBefore01(): Int = 1
fun kotlinPadBefore02(): Int = 2
fun kotlinPadBefore03(): Int = 3
fun kotlinPadBefore04(): Int = 4
fun kotlinPadBefore05(): Int = 5
fun kotlinPadBefore06(): Int = 6
fun kotlinPadBefore07(): Int = 7
fun kotlinPadBefore08(): Int = 8
fun kotlinPadBefore09(): Int = 9
fun kotlinPadBefore10(): Int = 10
fun kotlinPadBefore11(): Int = 11
fun kotlinPadBefore12(): Int = 12
fun kotlinPadBefore13(): Int = 13
fun kotlinPadBefore14(): Int = 14
fun kotlinPadBefore15(): Int = 15
fun kotlinPadBefore16(): Int = 16
fun kotlinPadBefore17(): Int = 17
fun kotlinPadBefore18(): Int = 18
fun kotlinPadBefore19(): Int = 19
fun kotlinPadBefore20(): Int = 20
fun kotlinPadBefore21(): Int = 21
fun kotlinPadBefore22(): Int = 22
fun kotlinPadBefore23(): Int = 23
fun kotlinPadBefore24(): Int = 24
fun kotlinPadBefore25(): Int = 25
fun kotlinPadBefore26(): Int = 26
fun kotlinPadBefore27(): Int = 27
fun kotlinPadBefore28(): Int = 28
fun kotlinPadBefore29(): Int = 29

fun kotlinEasy(value: Int): Int = value + 1
fun useKotlinEasy(): Int = kotlinEasy(4)

fun String.normalizedLabel(prefix: String): String {
    return "$prefix:${trim().lowercase()}"
}

suspend fun <T : WorkItem> kotlinComplex(
    items: List<T>,
    maxRetries: Int,
    emitter: WorkEmitter,
    transform: suspend (T) -> String,
): Map<String, Int> where T : Comparable<T> {
    require(maxRetries >= 0) { "maxRetries must be non-negative" }

    val ordered = items
        .asSequence()
        .filter { it.enabled }
        .sortedWith(compareByDescending<T> { it.priority }.thenBy { it.id })
        .toList()
    val attemptsById = linkedMapOf<String, Int>()

    for (item in ordered) {
        var attempt = 0
        var completed = false
        while (!completed && attempt <= maxRetries) {
            attempt += 1
            try {
                val rendered = transform(item).normalizedLabel("work")
                emitter.emit("${item.id}:$rendered")
                attemptsById[item.id] = attempt
                completed = true
            } catch (failure: IllegalStateException) {
                if (attempt > maxRetries) {
                    emitter.emit("${item.id}:failed:${failure.message}")
                    throw failure
                }
            }
        }
    }

    return attemptsById
}

suspend fun runKotlinComplex(items: List<WorkItem>, emitter: WorkEmitter): Map<String, Int> {
    return kotlinComplex(items, 3, emitter) { item ->
        "${item.id}:${item.priority}"
    }
}

fun kotlinPadAfter00(): Int = 0
fun kotlinPadAfter01(): Int = 1
fun kotlinPadAfter02(): Int = 2
fun kotlinPadAfter03(): Int = 3
fun kotlinPadAfter04(): Int = 4
fun kotlinPadAfter05(): Int = 5
fun kotlinPadAfter06(): Int = 6
fun kotlinPadAfter07(): Int = 7
fun kotlinPadAfter08(): Int = 8
fun kotlinPadAfter09(): Int = 9
fun kotlinPadAfter10(): Int = 10
fun kotlinPadAfter11(): Int = 11
fun kotlinPadAfter12(): Int = 12
fun kotlinPadAfter13(): Int = 13
fun kotlinPadAfter14(): Int = 14
fun kotlinPadAfter15(): Int = 15
fun kotlinPadAfter16(): Int = 16
fun kotlinPadAfter17(): Int = 17
fun kotlinPadAfter18(): Int = 18
fun kotlinPadAfter19(): Int = 19
fun kotlinPadAfter20(): Int = 20
fun kotlinPadAfter21(): Int = 21
fun kotlinPadAfter22(): Int = 22
fun kotlinPadAfter23(): Int = 23
fun kotlinPadAfter24(): Int = 24
fun kotlinPadAfter25(): Int = 25
fun kotlinPadAfter26(): Int = 26
fun kotlinPadAfter27(): Int = 27
fun kotlinPadAfter28(): Int = 28
fun kotlinPadAfter29(): Int = 29
