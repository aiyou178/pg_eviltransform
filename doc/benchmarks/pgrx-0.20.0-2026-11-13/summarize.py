from pathlib import Path
import re
import statistics

root = Path('/tmp/bench-results')
for major in (18, 19):
    print(f'PostgreSQL {major}')
    for kind in ('coordinates', 'jenks'):
        series = {}
        for phase in ('baseline', 'current'):
            paths = [root / f'{phase}-pg{major}' / f'{kind}-{i}.txt' for i in range(1, 6)]
            if not all(p.exists() for p in paths):
                continue
            runs = [list(map(float, re.findall(r'Execution Time: ([0-9.]+) ms', p.read_text()))) for p in paths]
            expected = 4 if kind == 'coordinates' else 3
            if any(len(run) != expected for run in runs):
                continue
            series[phase] = list(zip(*runs))
        if len(series) == 2:
            for index, (before, after) in enumerate(zip(series['baseline'], series['current'])):
                old, new = statistics.median(before), statistics.median(after)
                print(f'{kind}[{index}]: {old:.3f} -> {new:.3f} ms; {(1-new/old)*100:.1f}% reduction; ranges {min(before):.3f}-{max(before):.3f}, {min(after):.3f}-{max(after):.3f}')
