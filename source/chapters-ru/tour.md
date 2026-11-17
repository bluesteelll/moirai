---
title: Одна сессия от начала до конца
nav: Пример сессии
description: Пошаговый проход по одному вымышленному проекту — от moirai init до слитой параллельной линии — со спроектированным выводом командной строки.
---

# Одна сессия от начала до конца

В этом обзоре один небольшой вымышленный проект проходит через рабочий день: вы создаёте хранилище, планируете задачи, передаёте одну из них агенту, записываете полученные знания, восстанавливаетесь после записи, проигравшей гонку, и сливаете параллельную линию работы.

> [!NOTE]
> **Спроектированный вывод, а не запись сеанса**
>
> Moirai ещё не выпущена, и установить её пока нельзя. Каждая команда и каждый флаг на этой странице входят в её спроектированную командную строку, а сессии следуют формам вывода, которые задаёт спецификация. Несколько строк, формулировку которых она оставляет открытой, даны для иллюстрации: например, сообщение `init`, строки захвата задачи и открытия линии, тела брифа и пака. Проект, id, идентификаторы коммитов, время и пути придуманы для примера.

## Проект {#the-project}

tinyhttp — небольшой сервер HTTP/1.1. Его код лежит в git-репозитории `/work/tinyhttp`. Цель на сегодня — поддержка keep-alive: одно TCP-соединение обслуживает несколько запросов подряд.

Работу делают двое. Вы выступаете [оркестратором](glossary.html#term-orchestrator): планируете, проверяете и сливаете. Агент по имени `dev-1` пишет код в собственной оболочке. Позже параллельно идёт вторая линия работы — поддержка TLS.

На практике большую часть этих команд набирают агенты. Здесь вы набираете их сами, чтобы был виден каждый шаг. Читать вывод помогают два соглашения:

- Первая строка почти любого результата — **заголовок**, например `branch: main | rev 5 | 2 rows`. В заголовке ветка всегда стоит первой. `rev` — общий для всего хранилища [порядковый номер](glossary.html#term-rev) коммита, из которого прочитан результат; там, где команда ждёт [ревизию](glossary.html#term-revision), этот коммит записывается как `s5`.
- Каждая запись в выводе начинается со своего id, например `#3`, и занимает одну строку.

## Создаём хранилище {#create-the-store}

```console
$ cd /work/tinyhttp
$ moirai init
init: new store /work/tinyhttp/.git/moirai | branch main | rev 0
```

Внутри git-репозитория `moirai init` кладёт [хранилище](glossary.html#term-store) в общий каталог git, здесь это `.git/moirai`. Этот каталог лежит вне любого рабочего дерева, поэтому `git add -A` никогда не подхватит файлы хранилища, а каждое связанное рабочее дерево (worktree) репозитория находит одно и то же хранилище. Вне репозитория `init` создаёт вместо этого `./.moirai/`. У нового хранилища одна ветка, `main`, и пока ни одного коммита.

Команды, которые не читают ветку, например `init`, `doctor` или `config`, заголовка не печатают. Их первая строка начинается с имени самой команды.

Теперь попробуйте добавить первую задачу:

```console
$ moirai add task 'Keep-alive support'
error[E406 role_policy]: this write needs a lease
  = help: an orchestrator presents its session lease with --lease (mint it once per session: moirai claim --role orchestrator --session)
$ moirai claim --role orchestrator --session --agent orch
branch: main | rev 0
1 CALL tx.claim(role: 'orchestrator'): lease L-1 holder orch, expires 2026-11-16T21:30Z
$ export MOIRAI_LEASE=L-1
```

Запись отклонена с кодом выхода 6. В Moirai право на запись даёт только [аренда (lease)](glossary.html#term-lease), которую предъявляет вызывающий. Вызывающий без аренды может только записывать находки, заметки и вопросы через MCP-инструмент `remember`. Ошибка называет ровно одно исправление, и это исправление — следующая команда.

Сессионная аренда `L-1` даёт своему держателю права оркестратора. Когда вы работаете через Claude Code или Codex с установленными хуками Moirai, эту аренду выпускает за вас хук начала сессии. В простом терминале вы выпускаете её один раз и кладёте в `MOIRAI_LEASE`, чтобы последующие команды предъявляли её без `--lease`. Когда нет сессии харнесса, к которой можно привязать аренду, она живёт 12 часов, и каждая запись, которая её предъявляет, продлевает её. Виды аренд описаны в главе [«Работа с агентами»](agents.html#leases-and-roles).

## Планируем работу {#plan-the-work}

```console
$ moirai add task 'Keep-alive support'
branch: main | rev 0 -> 1 | committed c5e2b8a01 | lease L-1
1 CREATE task #1 "Keep-alive support"
affected: newly ready #1
$ moirai add task 'Parse the Connection header' --parent 1
branch: main | rev 1 -> 2 | committed c0d94f7e2 | lease L-1
1 CREATE task #2 "Parse the Connection header" UNDER #1
affected: newly ready #2; #1
$ moirai add task 'Reuse the connection in the accept loop' --parent 1 --blocked-by 2
branch: main | rev 2 -> 3 | committed c8b13c6d4 | lease L-1
1 CREATE task #3 "Reuse the connection in the accept loop" UNDER #1
2 CREATE #2 -[:BLOCKS]-> #3
affected: #2 #3
```

Каждая запись — это один коммит. Заголовок показывает порядковый номер до и после, короткий id коммита и аренду, которую предъявила запись. Полный id коммита — это `c` и 64 шестнадцатеричные цифры, хеш коммита; в результатах показаны `c` и первые восемь цифр. Дальше идёт по строке на каждый оператор: каждая команда записи — именованная мутация [Lachesis](lachesis.html), языка запросов Moirai, поэтому результат показывает на этом языке, что она сделала. Последняя строка перечисляет id, у которых изменилось производное состояние; `newly ready` называет задачи, которые теперь можно начинать.

После второй команды `#1` больше не готова к работе. Задача с дочерними задачами — это контейнер. Она никогда не попадает в число готовых, поэтому напрямую её не выдают ни одному агенту; агенты работают над её дочерними задачами.

Здесь видны две привычки командной строки. Короткие названия пишутся в одинарных кавычках. Id в аргументах пишутся без решётки (`--parent 1`, а не `--parent #1`), потому что в Git Bash и PowerShell `#` начинает комментарий.

```console
$ moirai add task 'Idle timeout for kept-alive sockets' --parent 1 --blocked-by 3
branch: main | rev 3 -> 4 | committed c2a9f05b7 | lease L-1
1 CREATE task #4 "Idle timeout for kept-alive sockets" UNDER #1
2 CREATE #3 -[:BLOCKS]-> #4
affected: #3 #4
$ moirai add task 'Keep-alive integration tests' --parent 1 --blocked-by 3
branch: main | rev 4 -> 5 | committed c7c41e3d8 | lease L-1
1 CREATE task #5 "Keep-alive integration tests" UNDER #1
2 CREATE #3 -[:BLOCKS]-> #5
affected: #5
$ moirai tree 1
branch: main | rev 5 | 5 rows
#1 task open P2 "Keep-alive support" children:0/4
  #2 task open P2 "Parse the Connection header" parent:#1
  #3 task open P2 "Reuse the connection in the accept loop" parent:#1 blockers:#2 BLOCKED
  #4 task open P2 "Idle timeout for kept-alive sockets" parent:#1 blockers:#3 BLOCKED
  #5 task open P2 "Keep-alive integration tests" parent:#1 blockers:#3 BLOCKED
```

Каждая строка `tree` — это **строка узла**: в этом формате печатает большинство команд чтения (некоторые, например `changes` ниже, печатают строки своего вида). В ней идут id, вид, статус, приоритет (по умолчанию `P2`), название в кавычках, затем ключи вроде `parent:`, `blockers:` (открытые блокеры) и `children:` (сколько выполнено из общего числа) и в конце флаги заглавными буквами. Названия всегда стоят в кавычках, потому что это текст, который написали люди и агенты.

<figure class="diagram">
<svg viewBox="0 0 720 232" role="img" aria-labelledby="tour-d1-t">
  <title id="tour-d1-t">План keep-alive на rev 5: четыре подзадачи под задачей 1, связанные рёбрами blocks</title>
  <defs><marker id="tour-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <rect class="d-box-muted" x="16" y="16" width="520" height="200" rx="8"/>
  <text class="d-text-strong" x="276" y="44" text-anchor="middle">#1 Поддержка keep-alive (контейнер)</text>
  <rect class="d-box-accent" x="40" y="100" width="136" height="72" rx="8"/>
  <text class="d-text-strong" x="108" y="124" text-anchor="middle">#2</text>
  <text class="d-text-small" x="108" y="146" text-anchor="middle">разбор заголовка</text>
  <text class="d-text-small" x="108" y="162" text-anchor="middle">Connection</text>
  <rect class="d-box" x="208" y="100" width="136" height="72" rx="8"/>
  <text class="d-text-strong" x="276" y="124" text-anchor="middle">#3</text>
  <text class="d-text-small" x="276" y="146" text-anchor="middle">переиспользование</text>
  <text class="d-text-small" x="276" y="162" text-anchor="middle">соединения</text>
  <rect class="d-box" x="376" y="64" width="152" height="56" rx="8"/>
  <text class="d-text-strong" x="452" y="88" text-anchor="middle">#4</text>
  <text class="d-text-small" x="452" y="108" text-anchor="middle">тайм-аут простоя</text>
  <rect class="d-box" x="376" y="144" width="152" height="56" rx="8"/>
  <text class="d-text-strong" x="452" y="168" text-anchor="middle">#5</text>
  <text class="d-text-small" x="452" y="188" text-anchor="middle">интеграционные тесты</text>
  <path class="d-edge" d="M176,136 L206,136" marker-end="url(#tour-arrow)"/>
  <path class="d-edge" d="M344,126 L374,98" marker-end="url(#tour-arrow)"/>
  <path class="d-edge" d="M344,146 L374,168" marker-end="url(#tour-arrow)"/>
  <text class="d-text-strong" x="632" y="80" text-anchor="middle">На rev 5</text>
  <text class="d-text-small" x="632" y="106" text-anchor="middle">готова к работе: #2</text>
  <text class="d-text-small" x="632" y="128" text-anchor="middle">блокируют: #2, #3</text>
  <text class="d-text-small" x="632" y="150" text-anchor="middle">заблокированы:</text>
  <text class="d-text-small" x="632" y="166" text-anchor="middle">#3, #4, #5</text>
  <text class="d-text-small" x="632" y="192" text-anchor="middle">стрелка = блокирует</text>
</svg>
<figcaption>План в виде графа. Стрелка от одной задачи к другой означает, что первая должна завершиться до начала второй. Начать сейчас можно только #2; #2 и #3 задерживают другую работу.</figcaption>
</figure>

## Что можно начинать и что задерживает работу {#ready-and-blocking}

```console
$ moirai ready
branch: main | rev 5 | live | 1 row
#2 task open P2 "Parse the Connection header" parent:#1
$ moirai blocking
branch: main | rev 5 | live | 2 rows
#2 task open P2 "Parse the Connection header" parent:#1
#3 task open P2 "Reuse the connection in the accept loop" parent:#1 blockers:#2 BLOCKED
$ moirai blocking --ids
#2
#3
```

`ready` перечисляет задачи, которые агент может начать прямо сейчас. Задача [готова к работе](glossary.html#term-ready), когда она открыта, у неё нет открытого [блокера](glossary.html#term-blocker) (ни своего, ни унаследованного от предка), она не контейнер и не отложена, на ней нет живой аренды другого агента и её не завершили и не удалили на другой ветке. Порядок фиксирован: сначала приоритет, затем id. `blocking` отвечает на обратный вопрос: какие незавершённые задачи задерживают другие незавершённые задачи.

Флаг `live` в обоих заголовках означает, что результат зависит от состояния во время выполнения, например от аренд, и поэтому описывает хранилище в данный момент. `--ids` печатает только id, по одному на строку и без заголовка, — для скриптов и конвейеров. Если такой список не помещается в свой предел в байтах ([«Бюджеты и футеры отброшенного»](agents.html#budgets-and-footers)), Moirai обрезает его по целым строкам, завершается с кодом 10 и пишет в stderr счётчик и курсор, так что при обрезке id никогда не теряются молча.

## Берём задачу и завершаем её {#claim-and-complete}

Агент `dev-1` берёт готовую задачу. В своей оболочке:

```console
$ moirai claim 2 --agent dev-1 --start
branch: main | rev 5 -> 6 | committed c41a7d0e2
1 CALL tx.claim([#2]): lease L-2 holder dev-1, expires 2026-11-16T09:55Z; status open->in_progress
affected: none
```

Захват даёт `dev-1` собственную аренду `L-2` на задачу `#2`. `--start` вдобавок переводит задачу в `in_progress` в том же коммите. Любой вызывающий может взять одну задачу для себя в роли разработчика или тестировщика; захваты для других ролей и захват многих задач сразу требуют оркестратора. Пока `dev-1` держит аренду, `#2` ни для кого другого не готова к работе.

Когда нет сессии харнесса, к которой можно привязать аренду, у неё 15-минутный срок. Каждая запись, которая предъявляет аренду, продлевает её, а во время долгой работы без записей её продлевает `moirai heartbeat L-2`. Если `dev-1` пропадёт, его аренда истечёт и не будет держать задачу вечно.

`dev-1` читает свой контекст-пак (он показан [ниже](#brief-and-pack)), пишет код, коммитит его в git как коммит `4f2c9e1` и отчитывается:

```console
$ moirai complete 2 --lease L-2 --outcome done --evidence commit:4f2c9e1 --summary - <<'EOF'
Connection header tokens compared without regard to case; 6 unit tests
EOF
branch: main | rev 6 -> 7 | committed c6d2a91f0 | lease L-2
1 CALL tx.complete(#2): status in_progress->done (resolution completed); lease L-2 released into settled
affected: newly ready #3 | markers: settled #2 (main)
```

Команда `complete` должна предъявить аренду задачи. Сводка, как и любой длинный текст, приходит на стандартный ввод через heredoc в кавычках, потому что оболочки по-разному обрабатывают кавычки ([«Аргументы, которые переживают любую оболочку»](agents.html#bodies-on-stdin)). Она становится сообщением коммита, а после неё добавляются доказательства (evidence). Исход может быть `done`, `failed` или `abandoned`; в каждом случае задача закрывается, но с разным итогом (resolution).

Аренда освобождается, а маркер `settled` фиксирует, что `#2` завершена на `main`. Другие ветки видят этот маркер, поэтому ни одна ветка не выдаст ту же задачу второму агенту. Результат также сообщает, что `#3` стала готова к работе, так что оркестратору не нужен лишний запрос, чтобы найти следующую задачу.

## Записываем правило, заметку и решение {#record-knowledge}

Работа над `#2` научила команду трём вещам. Вы их записываете:

```console
$ moirai rule --critical --applies-to 'role:developer,path:src/conn/**' --stdin <<'EOF'
Every socket read in src/conn has a timeout
EOF
branch: main | rev 7 -> 8 | committed c9e2f4a17 | lease L-1
1 CREATE rule #6 "Every socket read in src/conn has a timeout"
2 SET #6.text = $text
affected: none
$ moirai add note 'Clients send Connection: Keep-Alive in any letter case' --body - <<'EOF'
Seen in captures from curl, wrk and two browsers: "keep-alive", "Keep-Alive"
and "KEEP-ALIVE". Compare the header tokens without regard to case.
EOF
branch: main | rev 8 -> 9 | committed c3b8d1e64 | lease L-1
1 CREATE note #7 "Clients send Connection: Keep-Alive in any letter case"
2 SET #7.body = $body
affected: none
$ moirai decision --about 3 --stdin <<'EOF'
Reuse a connection only after the whole response is written
EOF
branch: main | rev 9 -> 10 | committed c6a0c9b38 | lease L-1
1 CREATE decision #8 "Reuse a connection only after the whole response is written"
2 SET #8.body = $text
3 CREATE #8 -[:ABOUT]-> #3
affected: none
```

*Вывод сокращён: опущен эффект, который каждая строка `SET` печатает после двоеточия (старое и новое значение поля).*

**Правило** — это указание, которому агенты обязаны следовать. `--applies-to` ограничивает его ролями и путями файлов; `--critical` гарантирует, что оно дойдёт до каждого агента, к которому относится. Значение-глоб взято в одинарные кавычки, потому что `*` без кавычек bash раскрывает, а zsh на нём останавливается с ошибкой. **Заметка** фиксирует факт; её тело приходит на стандартный ввод. **Решение** фиксирует сделанный выбор; здесь оно связано с задачей `#3` ребром `ABOUT`. В каждом операторе свободный текст передаётся как параметр, например `$text`, и никогда не вставляется в сам оператор. Правило хранит свою формулировку в отдельном поле `text`; остальные виды узлов хранят длинный текст в `body`. Опции `--stdin`, `--body -` и `--summary -` все читают текст со стандартного ввода; они принадлежат разным командам и заполняют разные поля.

Правила и решения создаются в статусе `proposed`, заметки — в статусе `active`. Предложенная запись показывается агентам, но с пометкой, что её ещё не проверили. Кто-то, обычно оркестратор, разбирает очередь:

```console
$ moirai q proposed
branch: main | rev 10 | 2 rows
#8 decision proposed "Reuse a connection only after the whole response is written"
#6 rule proposed critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
$ moirai set 6 --status active
branch: main | rev 10 -> 11 | committed c2d71f5a0 | lease L-1
1 SET #6.status = $status: status proposed->active
affected: none
```

`moirai q proposed` выполняет [именованный запрос](glossary.html#term-named-query) стандартной библиотеки; каждая команда чтения — такой запрос, поэтому `moirai ready` — то же самое, что `moirai q ready`. Правило теперь активно. Решение пока остаётся предложенным.

## Что видит агент: бриф и пак {#brief-and-pack}

На следующее утро в репозитории начинается новая сессия агента. Первое, что она видит о проекте, — [бриф](glossary.html#term-brief):

```console
$ moirai brief
moirai brief | branch main | rev 11 | 226/8,000 B
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
needs review: 1 proposed | newest #8 | list: moirai q proposed
```

*Вывод сокращён: показаны только некоторые строки брифа.*

Бриф — это дайджест всего хранилища для начала сессии: живые линии и их аренды, идущие прогоны, слияния, остановленные в staging, открытые вопросы к владельцу проекта, критические правила, новые вердикты, сломанные файловые ссылки и число записей, ждущих проверки. Его первая строка вместо заголовка начинается с имени команды и сообщает, сколько байт использовано и каков бюджет — по умолчанию 8 000 байт. С установленными хуками хук начала сессии кладёт бриф в контекст агента до его первого хода. Без хуков первый вызов агента — `moirai brief`, и результат тот же.

Прежде чем взяться за `#3`, `dev-1` запрашивает свой [контекст-пак](glossary.html#term-context-pack):

```console
$ moirai pack 3 --role developer
moirai pack #3 developer | branch main | rev 11 | 408/16,000 B | digest 11-5b0e7c42
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
#3 task open P2 "Reuse the connection in the accept loop" parent:#1
#1 task open P2 "Keep-alive support" children:1/4
#8 decision proposed "Reuse a connection only after the whole response is written" ~proposed
```

*Вывод сокращён: остальные строки заголовка пака опущены, а каждая запись показана в однострочной форме.*

Пак строится для одной задачи и одной роли. Для разработчика в нём есть правила, относящиеся к роли, задача и её предки, решения и разделы плана о задаче, подтверждённые находки, замеры, заметки по файлам, которыми владеет задача, и то, что изменилось после последнего пака этого агента. Маркер `~proposed` говорит агенту, что решение `#8` ещё не проверено.

Бюджет задаётся в байтах — по умолчанию 16 000 для разработчика, и здесь пак использует 408 из них. Пак побольше заполняет каждый класс до минимальной доли, сокращает записи, прежде чем что-либо отбросить, и никогда не обрезает запись посередине. Если что-то отброшено, и первая, и последняя строки говорят, сколько, и дают команду, которая выдаст остальное. Дайджест в первой строке идентифицирует то, что показал этот пак, поэтому `complete` позже может сообщить, что изменилось с тех пор, как агент его прочитал.

Когда оркестратор запускает `dev-1` как субагента в Claude Code или Codex, хук до первого хода агента вставляет короткий ролевой пак, не больше 3 000 байт, с критическими правилами. Тогда собственный `pack` агента перечисляет эти правила одной строкой id, а не повторяет их. Классы и бюджеты подробно описаны в главе [«Работа с агентами»](agents.html#packs-and-briefs).

## Ищем нужное: show и find {#show-and-find}

```console
$ moirai show 7 --full
branch: main | rev 11 | 1 row
#7 note active "Clients send Connection: Keep-Alive in any letter case"
  title: "Clients send Connection: Keep-Alive in any letter case"
  status: active
--- body #7 | 145 B | by orch rev 9 | untrusted text ---
Seen in captures from curl, wrk and two browsers: "keep-alive", "Keep-Alive"
and "KEEP-ALIVE". Compare the header tokens without regard to case.
--- end body #7 ---
```

*Вывод сокращён: `show` печатает строку для каждого поля заметки в порядке схемы; здесь показаны только две.*

`show` печатает строку узла, а затем по строке на каждое поле. С `--full` она добавляет тело в рамке с пометкой `untrusted text`: для читающего это данные, а не инструкция (см. [«Текст, написанный другими, — это данные»](agents.html#untrusted-text)).

Проходит какое-то время. `dev-1` берёт `#3` с новой арендой `L-3` (rev 12). Второй проход планирования добавляет задачу `#9` «Close idle connections after a timeout», которая блокирует `#5` (rev 13). Вы ищете всё, что касается тайм-аутов:

```console
$ moirai find timeout
branch: main | rev 13 | 3 rows
#4 task open P2 "Idle timeout for kept-alive sockets" parent:#1 blockers:#3 BLOCKED
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
#9 task open P2 "Close idle connections after a timeout" parent:#1
```

`find` принимает свободные слова и фильтры вроде `kind:task`, `status:open`, `prio:..1` или `done:false`; с `--bodies` поиск идёт и по телам. Результат выявляет проблему: `#9` описывает ту же работу, что и `#4`. Чуть позже вы её удалите.

## Запись, проигравшая гонку {#guarded-write}

Вы хотите поднять `#3` до приоритета P1, но только пока она ещё в работе: если она уже завершена, изменение бессмысленно. Вы не знаете, что `dev-1` завершил её минуту назад (rev 14).

```console
$ moirai set 3 priority=P1 --if-status in_progress
error[E401 expect_mismatch]: statement 1 matched 0 bindings, expected 1
  #3 now: status=done rev=14 (changed at c7e19b2d0 by dev-1 on main: "Reuse the connection after each response")
  nothing was written
  = help: re-read with moirai q show ids=3
$ echo $?
4
```

`--if-status` — это [охранное условие (guard)](glossary.html#term-cas-guard): запись проверяет его по вершине ветки, на которую она легла бы, в том же шаге, в котором выполняется сама запись. Условия бывают такие: `--if-rev N` (N — порядковый номер последнего изменения узла), `--if-status S` и `--if-holder H` (держатель аренды задачи). Если условие не выполнено, ничего не записывается, и команда завершается с кодом 4. Ошибка показывает текущее значение, кто его изменил, в каком коммите и с каким сообщением, и называет одно исправление.

Условия нужны потому, что агент не может убрать из своего контекстного окна то, что туда уже попало. Значение, прочитанное десять минут назад, сейчас может быть неверным. Защищённая запись превращает такое устаревшее знание в ясный отказ с фактами, которые нужны, чтобы решить заново. Текст любой ошибки укладывается в 600 байт.

## Удаление: сначала пробный запуск {#delete-dry-run}

```console
$ moirai rm 9 --replaced-by 4 --reason duplicate --dry-run
branch: main | rev 14 | tx (dry) | would commit 3 changes
1 DELETE #9 task "Close idle connections after a timeout" REPLACED BY #4
    #9 -[:BLOCKS]-> #5    re-pointed: #4 -[:BLOCKS]-> #5
+ #4 task edge BLOCKS BLOCKS #5 added
- #9 task edge BLOCKS BLOCKS #5 removed
~ #9 task existence task "Close idle connections after a timeout" deleted ("duplicate", replaced_by #4)
affected: #4 #5 | invariants ok
targets: 3f6c0a9e1b7d4c2a8e5f0b1d9c7a6e42
to apply: send the same TX without DRY, with IF TARGETS '3f6c0a9e1b7d4c2a8e5f0b1d9c7a6e42'
$ moirai rm 9 --replaced-by 4 --reason duplicate --yes
branch: main | rev 14 -> 15 | committed c1f8a6d23 | lease L-1
1 DELETE #9 task "Close idle connections after a timeout" REPLACED BY #4
    #9 -[:BLOCKS]-> #5    re-pointed: #4 -[:BLOCKS]-> #5
affected: #4 #5 | markers: deleted #9 (main)
```

Пробный запуск ничего не пишет и завершается с кодом 0. Он показывает, что затронет удаление: каждое ребро вместе с политикой, которая к нему применяется, изменения в виде строк диффа и проверку того, что все инварианты по-прежнему выполняются. Строка диффа содержит вид изменения (`+` добавлено, `-` удалено, `~` изменено), узел и его вид, часть узла, которая меняется (здесь `edge`), имя этой части (вид ребра `BLOCKS`) и то, что произошло (для ребра — его вид, его цель и `added` или `removed`). Здесь ребро «#9 блокирует #5» переходит к замене, так что `#5` теперь ждёт `#4`. Две последние строки дают дайджест множества целей изменения и говорят, как применить то же изменение блоком Lachesis; в командной строке его применяет та же команда с `--yes`. Исторические ссылки, например заметка, которая цитирует удалённый узел, остаются на месте и указывают на надгробие, а цитирующая запись помечается как подозрительная. Moirai никогда ни о чём не спрашивает; `rm` без `--yes` печатает пробный запуск.

После `--yes` узел `#9` становится [надгробием](glossary.html#term-tombstone): у него сохраняются id, причина и замена, любая ссылка на него отображается как `#9(deleted)`, а `moirai show 9` печатает надгробие и завершается с кодом 3. Маркер `deleted` сообщает другим веткам, что `#9` никому нельзя выдавать. Удалять узлы могут только оркестратор и владелец, и только через командную строку.

## Догоняем: изменения с ревизии {#changes}

```console
$ moirai changes --since 13
branch: main | rev 15 | 7 rows
rev 14 main #3 ~ status dev-1
rev 14 main #3 end lease L-3 dev-1
rev 14 main #3 settled marker s14 dev-1
rev 15 main #4 + edge BLOCKS orch
rev 15 main #9 - edge BLOCKS orch
rev 15 main #9 ~ existence task orch
rev 15 main #9 deleted marker s15 orch
```

В [ленте изменений](glossary.html#term-change-feed) по строке на изменение: порядковый номер его коммита, ветка, узел, операция, изменившаяся часть узла и тот, кто её изменил. События аренд и маркеры — тоже записи ленты. `--about 3,4` сужает ленту до нескольких узлов, `--for-agent A` оставляет только изменения, важные для одного агента (узлы, которые он взял, ждёт, писал или цитировал), а `--all` добавляет все остальные ветки. С установленными хуками та же лента доходит до агента короткой дельтой перед каждым промптом — не больше 600 байт и ничего, если ничего не изменилось.

## Параллельная линия {#parallel-lane}

Поддержкой TLS можно заниматься, пока продолжается работа над keep-alive. Обычной командой git вы создаёте рабочее дерево в `/work/tinyhttp-tls` на новой ветке git `tls` и открываете для него [линию (lane)](glossary.html#term-lane):

```console
$ moirai lane open tls --worktree /work/tinyhttp-tls --git-branch tls
branch: main | rev 15 -> 16 | committed c0b6e3f81 | lease L-1
lane #10 "tls" | lane/tls forked at c0b6e3f81 | /work/tinyhttp-tls bound to lane/tls
$ cd /work/tinyhttp-tls
$ moirai add task 'TLS listener on port 8443' --parent 1
branch: lane/tls | rev 16 -> 17 | committed c5f0d2a19 | lease L-1
1 CREATE task #11 "TLS listener on port 8443" UNDER #1
affected: newly ready #11
```

Линия связывает три вещи: ветку графа (`lane/tls`), рабочее дерево git и узел линии на `main`, который их записывает. Каталог рабочего дерева привязан к ветке, поэтому каждая команда, запущенная внутри него, читает и пишет `lane/tls`, и каждый заголовок сообщает об этом первым полем. Порядковые номера общие для всего хранилища, поэтому у коммитов одной ветки номера могут идти с пропусками.

Тем временем в основном рабочем дереве вы принимаете решение `#8` (rev 18). На линии:

```console
$ moirai ready
branch: lane/tls | rev 17 | live | 2 rows | behind main 1
#4 task open P2 "Idle timeout for kept-alive sockets" parent:#1
#11 task open P2 "TLS listener on port 8443" parent:#1
$ moirai sync
branch: lane/tls | rev 17 -> 19 | committed c8d3a0b75 | lease L-1
sync main into lane/tls: landed | 0 conflicts | 0 violations
$ moirai merge lane/tls --into main
branch: main | rev 18 -> 20 | committed c2f9e6a48 | lease L-1
merge lane/tls into main: landed | 0 conflicts | 0 violations
```

`behind main 1` в заголовке предупреждает, что линии не хватает одного коммита из `main`. `sync` сливает `main` в линию; `merge` переносит работу линии в `main`, а если `main` ушла вперёд, сначала сама выполняет синхронизацию. Слияние типизировано по полям и по рёбрам. Конфликт становится значением, которое разрешает агент или человек, а изменение, которое нарушило бы инвариант, останавливает слияние на отдельной ветке staging, пока кто-нибудь не примет решение.

Moirai сливает граф, а код по-прежнему сливает git (`git merge tls`). Затем `moirai lane close tls` отмечает линию как слитую и снимает привязку каталога. Линии, конфликты, staging и отмена подробно разобраны в главе [«Ветки, слияния и история»](versions.html).

## Что показал обзор {#what-you-saw}

- Одно хранилище на репозиторий — вне любого рабочего дерева и общее для всех рабочих деревьев.
- Задачи с родителями и блокерами и прямые ответы на два вопроса: что можно начинать (`ready`) и что задерживает работу (`blocking`).
- Права на запись, которые дают только аренды, — от `claim` до `complete`.
- Правила, заметки и решения с очередью проверки для предложенных записей.
- Контекст для агентов в пределах бюджета: бриф в начале сессии и пак на каждую задачу.
- Защищённые записи, которые отклоняют устаревшие изменения и показывают текущее значение.
- Удаления, которые сначала показывают свои последствия и оставляют после себя надгробия.
- Ленту изменений и параллельную линию, которая синхронизируется и сливается, как ветка git.

Дальше: глава [«Работа с агентами»](agents.html) объясняет, через какие интерфейсы работают агенты, а глава [Lachesis](lachesis.html) показывает язык запросов, который стоит за каждой командой.
