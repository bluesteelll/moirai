---
title: Moirai
nav: Главная
description: Moirai — версионируемая графовая база данных для менеджмента воркфлоу ИИ-агентов, написанная на Rust.
---

# Moirai

Moirai — версионируемая графовая база данных для менеджмента воркфлоу ИИ-агентов, написанная на Rust.

Она даёт всем агентам проекта одно место для задач, памяти проекта, правил и решений. Всё это версионируется с ветками, как в git, и доступно всем агентам одновременно.

Основные термины собраны в [глоссарии](glossary.html).

## Что делает Moirai {#what-it-does}

Moirai хранит все данные в одном типизированном графе. Задача, вопрос, правило, решение, заметка, находка ревью и исходный файл — всё это [узлы](glossary.html#term-node), а информативные связи между ними — [рёбра](concepts.html#edges). В базовой схеме 25 видов рёбер, например: подзадача принадлежит задаче, одна задача должна завершиться раньше другой, решение отвечает на вопрос, новое правило заменяет старое, находка ревью относится к проверенному коду, заметка привязана к месту в исходном файле.  

Движок проверяет каждую запись по схеме и сам вычисляет то, что следует из графа, например какие задачи можно начинать сейчас, так что никому не приходится вести такие списки вручную. Чтобы получать из графа ответы, у Moirai есть собственный язык запросов — [Lachesis](lachesis.html). Один запрос может проходить сразу по множеству связей, и агенту не нужно перебирать файлы и заметки вручную.

Вокруг графа — всё, что нужно агентам для параллельной работы: [аренды](glossary.html#term-lease) задач, ветки со слиянием, копия в git, ссылки на файлы, которые не ломаются при переносе, и язык запросов. Карточки ниже ведут к главам о каждой части.

<div class="cards">
<a class="card" href="why.html"><span class="card-title">Зачем нужна Moirai</span><span class="card-text">Агенты забывают всё между сессиями, планы в Markdown теряют согласованность, ссылки на файлы устаревают. Проблема и идея.</span></a>
<a class="card" href="concepts.html"><span class="card-title">Граф</span><span class="card-text">Задачи, правила, решения и находки как типизированные узлы. Движок вычисляет, какие задачи готовы к работе.</span></a>
<a class="card" href="tour.html"><span class="card-title">Пример сессии</span><span class="card-text">Одна спроектированная сессия — от <code>moirai init</code> до слитой параллельной линии.</span></a>
<a class="card" href="versions.html"><span class="card-title">Ветки и слияния</span><span class="card-text">Собственный контроль версий: ветка на каждую линию, типизированное слияние, конфликты хранятся как данные.</span></a>
<a class="card" href="git-image.html"><span class="card-title">Сохранение в git</span><span class="card-text">Весь граф в виде читаемых текстовых файлов в git-репозитории и обратно.</span></a>
<a class="card" href="file-links.html"><span class="card-title">Файловые ссылки</span><span class="card-text">Указатели на файлы проекта, которые следуют за переименованиями, перемещениями и правками.</span></a>
<a class="card" href="lachesis.html"><span class="card-title">Lachesis</span><span class="card-text">Язык запросов в стиле Cypher, на котором построены все чтения графа и большинство записей.</span></a>
<a class="card" href="agents.html"><span class="card-title">Работа с агентами</span><span class="card-text">CLI, MCP-сервер, skills и хуки, а также контекст, подобранный по размеру под каждую задачу.</span></a>
</div>

## Первый взгляд {#first-look}

Вот две команды из [примера сессии](tour.html) на небольшом вымышленном проекте. Первая спрашивает, какие задачи агент может начать сейчас. Вторая позволяет агенту `dev-1` взять одну из них.

```console
$ moirai ready
branch: main | rev 5 | live | 1 row
#2 task open P2 "Parse the Connection header" parent:#1
$ moirai claim 2 --agent dev-1 --start
branch: main | rev 5 -> 6 | committed c41a7d0e2
1 CALL tx.claim([#2]): lease L-2 holder dev-1, expires 2026-11-16T09:55Z; status open->in_progress
affected: none
```

*Спроектированный вывод.*

Первая строка каждого результата называет ветку и ревизию, из которой он прочитан, а команда записи показывает ревизию до и после. Каждая запись данных занимает одну строку и начинается со своего короткого id. Ответ на `ready` движок вывел из дерева задач, блокеров и аренд. Захват даёт `dev-1` аренду на задачу `#2`. Её видит каждая ветка хранилища, поэтому ту же работу никакому другому агенту не выдадут.

## Общая картина {#big-picture}

<figure class="diagram">
<svg viewBox="0 0 720 564" role="img" aria-labelledby="index-d1-t">
  <title id="index-d1-t">Общая картина: агенты в своих харнессах работают с бинарником moirai через CLI, MCP-сервер и хуки; внутри него находятся Lachesis и движок Klotho; движок работает с файлами проекта, хранилищем и git-образом, который передаётся в удалённый git-репозиторий</title>
  <defs><marker id="index-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <path class="d-box-muted" d="M24,12 H696 A8,8 0 0 1 704,20 V84 A8,8 0 0 1 696,92 H24 A8,8 0 0 1 16,84 V20 A8,8 0 0 1 24,12 Z"/>
  <text class="d-text-small" x="32" y="27" text-anchor="start" dominant-baseline="middle">AI-агенты для программирования в своих харнессах; skills учат их командам</text>
  <path class="d-box" d="M48,40 H208 A8,8 0 0 1 216,48 V72 A8,8 0 0 1 208,80 H48 A8,8 0 0 1 40,72 V48 A8,8 0 0 1 48,40 Z"/>
  <text class="d-text" x="128" y="60" text-anchor="middle" dominant-baseline="middle">Claude Code</text>
  <path class="d-box" d="M280,40 H440 A8,8 0 0 1 448,48 V72 A8,8 0 0 1 440,80 H280 A8,8 0 0 1 272,72 V48 A8,8 0 0 1 280,40 Z"/>
  <text class="d-text" x="360" y="60" text-anchor="middle" dominant-baseline="middle">Codex</text>
  <path class="d-box" d="M512,40 H672 A8,8 0 0 1 680,48 V72 A8,8 0 0 1 672,80 H512 A8,8 0 0 1 504,72 V48 A8,8 0 0 1 512,40 Z"/>
  <text class="d-text" x="592" y="60" text-anchor="middle" dominant-baseline="middle">другие харнессы</text>
  <path class="d-edge" d="M128,80 L128,124" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,80 L360,124" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M592,80 L592,124" marker-end="url(#index-arrow)"/>
  <path class="d-box-muted" d="M24,124 H696 A8,8 0 0 1 704,132 V364 A8,8 0 0 1 696,372 H24 A8,8 0 0 1 16,364 V132 A8,8 0 0 1 24,124 Z"/>
  <text class="d-text-strong" x="32" y="143" text-anchor="start" dominant-baseline="middle">moirai: один бинарник, без демона</text>
  <path class="d-box" d="M48,160 H208 A8,8 0 0 1 216,168 V208 A8,8 0 0 1 208,216 H48 A8,8 0 0 1 40,208 V168 A8,8 0 0 1 48,160 Z"/>
  <text class="d-text" x="128" y="181" text-anchor="middle" dominant-baseline="middle">CLI</text>
  <text class="d-text-small" x="128" y="201" text-anchor="middle" dominant-baseline="middle">moirai &lt;verb&gt;</text>
  <path class="d-box" d="M280,160 H440 A8,8 0 0 1 448,168 V208 A8,8 0 0 1 440,216 H280 A8,8 0 0 1 272,208 V168 A8,8 0 0 1 280,160 Z"/>
  <text class="d-text" x="360" y="181" text-anchor="middle" dominant-baseline="middle">MCP-сервер</text>
  <text class="d-text-small" x="360" y="201" text-anchor="middle" dominant-baseline="middle">stdio, 10 инструментов</text>
  <path class="d-box" d="M512,160 H672 A8,8 0 0 1 680,168 V208 A8,8 0 0 1 672,216 H512 A8,8 0 0 1 504,208 V168 A8,8 0 0 1 512,160 Z"/>
  <text class="d-text" x="592" y="181" text-anchor="middle" dominant-baseline="middle">Хуки (опционально)</text>
  <text class="d-text-small" x="592" y="201" text-anchor="middle" dominant-baseline="middle">при сбое не блокируют</text>
  <path class="d-edge" d="M128,216 L128,240" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,216 L360,240" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M592,216 L592,240" marker-end="url(#index-arrow)"/>
  <path class="d-box" d="M48,240 H672 A8,8 0 0 1 680,248 V272 A8,8 0 0 1 672,280 H48 A8,8 0 0 1 40,272 V248 A8,8 0 0 1 48,240 Z"/>
  <text class="d-text" x="360" y="260" text-anchor="middle" dominant-baseline="middle">Lachesis: запросы и мутации графа</text>
  <path class="d-edge" d="M360,280 L360,304" marker-end="url(#index-arrow)"/>
  <path class="d-box-accent" d="M48,304 H672 A8,8 0 0 1 680,312 V348 A8,8 0 0 1 672,356 H48 A8,8 0 0 1 40,348 V312 A8,8 0 0 1 48,304 Z"/>
  <text class="d-text-strong" x="360" y="323" text-anchor="middle" dominant-baseline="middle">Klotho: движок хранения</text>
  <text class="d-text-small" x="360" y="343" text-anchor="middle" dominant-baseline="middle">журнал, сегменты, блокировки, ветки и история</text>
  <path class="d-edge" d="M136,356 L136,412" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,356 L360,412" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <path class="d-edge-dashed" d="M584,356 L584,412" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <text class="d-text-small" x="146" y="392" text-anchor="start" dominant-baseline="middle">файловые ссылки</text>
  <text class="d-text-small" x="370" y="392" text-anchor="start" dominant-baseline="middle">журнал и сегменты</text>
  <text class="d-text-small" x="594" y="392" text-anchor="start" dominant-baseline="middle">экспорт / импорт</text>
  <path class="d-box" d="M48,412 H224 A8,8 0 0 1 232,420 V460 A8,8 0 0 1 224,468 H48 A8,8 0 0 1 40,460 V420 A8,8 0 0 1 48,412 Z"/>
  <text class="d-text" x="136" y="432" text-anchor="middle" dominant-baseline="middle">файлы проекта</text>
  <text class="d-text-small" x="136" y="452" text-anchor="middle" dominant-baseline="middle">в каждом рабочем дереве</text>
  <path class="d-box" d="M272,412 H448 A8,8 0 0 1 456,420 V460 A8,8 0 0 1 448,468 H272 A8,8 0 0 1 264,460 V420 A8,8 0 0 1 272,412 Z"/>
  <text class="d-text" x="360" y="432" text-anchor="middle" dominant-baseline="middle">каталог хранилища</text>
  <text class="d-text-small" x="360" y="452" text-anchor="middle" dominant-baseline="middle">&lt;git-common-dir&gt;/moirai/</text>
  <path class="d-box" d="M496,412 H672 A8,8 0 0 1 680,420 V460 A8,8 0 0 1 672,468 H496 A8,8 0 0 1 488,460 V420 A8,8 0 0 1 496,412 Z"/>
  <text class="d-text" x="584" y="432" text-anchor="middle" dominant-baseline="middle">репозиторий образа</text>
  <text class="d-text-small" x="584" y="452" text-anchor="middle" dominant-baseline="middle">один файл .moi на узел</text>
  <path class="d-edge-dashed" d="M584,468 L584,500" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <text class="d-text-small" x="574" y="485" text-anchor="end" dominant-baseline="middle">push / pull через git</text>
  <path class="d-box" d="M496,500 H672 A8,8 0 0 1 680,508 V540 A8,8 0 0 1 672,548 H496 A8,8 0 0 1 488,540 V508 A8,8 0 0 1 496,500 Z"/>
  <text class="d-text" x="584" y="517" text-anchor="middle" dominant-baseline="middle">удалённый репозиторий</text>
  <text class="d-text-small" x="584" y="535" text-anchor="middle" dominant-baseline="middle">любой git-хостинг</text>
  <text class="d-text-muted" x="40" y="512" text-anchor="start" dominant-baseline="middle">пунктир: явные необязательные шаги</text>
  <text class="d-text-muted" x="40" y="534" text-anchor="start" dominant-baseline="middle">одно хранилище на репозиторий</text>
</svg>
<figcaption>Агенты обращаются к Moirai через командную строку, MCP-сервер или необязательные хуки. Командам их учат skills — короткие файлы с инструкциями, которые устанавливаются в каждый харнесс; сам бинарник устанавливается отдельно. Команды читают граф запросами Lachesis и меняют его мутациями Lachesis, а выполняет их Klotho — движок хранения внутри единственного бинарника <code>moirai</code>. Klotho держит все данные в одном хранилище на репозиторий, общем для всех рабочих деревьев, а его файловые ссылки указывают на файлы проекта. Текстовая копия хранилища в отдельном git-репозитории — это git-образ; обычный <code>git</code> может отправить его в любой удалённый репозиторий.</figcaption>
</figure>

> [!NOTE]
> На схеме встречаются три имени. **Moirai** — это продукт, а `moirai` — его единственная команда. **[Klotho](glossary.html#term-klotho)** — движок хранения, библиотека внутри этого бинарника. **[Lachesis](glossary.html#term-lachesis)** — язык запросов. В греческих мифах мойры (Moirai) — три богини судьбы: Клото (Klotho) прядёт нить, а Лахесис (Lachesis) её отмеряет.




## Как читать эту книгу {#how-to-read}

Главы опираются друг на друга, но каждую можно читать отдельно. Если у вас есть десять минут, прочитайте первую главу и пример сессии.

1. [Зачем нужна Moirai](why.html): какую проблему она решает, её требования, четыре приоритета и то, чем она сознательно не является.
2. [Граф](concepts.html): узлы, виды, статусы, рёбра, производное состояние вроде «ready», удаление и аренды.
3. [Одна сессия от начала до конца](tour.html): один вымышленный проект, команда за командой, со спроектированным выводом.
4. [Ветки, слияния и история](versions.html): собственный контроль версий Moirai и то, как сливаются параллельные линии.
5. [Сохранение в git](git-image.html): как граф записывается в git-репозиторий в виде текста и читается обратно.
6. [Файловые ссылки, устойчивые к перемещениям](file-links.html): указатели на файлы проекта, которые следуют за переименованиями и правками.
7. [Lachesis, язык запросов](lachesis.html): запросы, версии внутри запросов, защищённые записи и бенчмарк точности.
8. [Работа с агентами](agents.html): CLI, MCP-сервер, skills, хуки, контекст-паки и аренды.
9. [Под капотом: Klotho](engine.html): движок хранения, устойчивость к сбоям и бюджеты производительности.
10. [Как это строится и где проект сейчас](building.html): спецификация, эталонная модель, тесты, дорожная карта и статус.
11. [Вопросы и ответы](faq.html): короткие ответы на вопросы, которые задают первыми.
12. [Глоссарий](glossary.html): все термины книги, каждый со ссылкой на главу, где он важен.

---

Moirai — проект с открытым исходным кодом под лицензией Apache License 2.0. Исходный код, проектные документы и спецификация лежат на GitHub: [github.com/bluesteelll/moirai](https://github.com/bluesteelll/moirai).
