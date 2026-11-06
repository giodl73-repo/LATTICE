"""Complete portable controls and independent public-fixture expectations."""
import json

ORIGINAL_STREAM_SHA256 = '573eb7a32d12b0faa71eeeb68e5b17d78887829eda5bf31772309bbf59681cdf'


def controls():
    result = [dict(left=a, right=b, budget=n, operation=op)
              for a in range(32) for b in range(32)
              for n in range(1, 9) for op in ('join', 'meet')]
    for key, values in [('left', [32, 255, 4294967295]),
                        ('right', [32, 255, 4294967295]),
                        ('budget', [0, 9, 4294967295]),
                        ('operation', ['', 'JOIN', 'close', '∧', 'join\0'])]:
        for value in values:
            item = dict(left=3, right=6, budget=8, operation='join')
            item[key] = value
            result.append(item)
    return result


def examples():
    selected = [(3, 6, 8, 'join'), (3, 6, 8, 'meet'),
                (31, 31, 3, 'join'), (31, 31, 3, 'meet'),
                (31, 0, 4, 'join'), (0, 31, 4, 'join'),
                (31, 31, 8, 'join'), (0, 0, 8, 'meet'),
                (21, 10, 5, 'join'), (15, 30, 6, 'meet'),
                (1, 16, 4, 'join'), (2, 6, 8, 'join'),
                (3, 6, 2, 'join'), (32, 6, 8, 'join')]
    return [dict(left=a, right=b, budget=n, operation=op)
            for a, b, n, op in selected]


def encoded_controls(items):
    return b''.join((json.dumps(c, ensure_ascii=False, separators=(',', ':'))+'\n').encode()
                    for c in items)


def validate(items, lines):
    required = {'source', 'policy', 'receipt'}
    successful = budget_failures = invalid = 0
    for c, line in zip(items, lines, strict=True):
        result = json.loads(line)
        if (c['left'] > 31 or c['right'] > 31 or not 1 <= c['budget'] <= 8
                or c['operation'] not in ('join', 'meet')):
            assert result == {'Err': 'Choose five sample items, a 1–8 grain budget, and meet or join'}
            invalid += 1
            continue
        if c['budget'] < 3:
            assert result == {'Err': 'required closure grains exceed grain budget'}
            budget_failures += 1
            continue
        successful += 1
        value = json.loads(result['Ok'])
        assert value['schema'] == 'lattice.browser.v1' and value['synthetic'] is True
        assert value['operation'] == c['operation'] and value['budget'] == c['budget']
        retained_inputs = []
        for side in ('left', 'right'):
            selected = ['optional-'+str(i) for i in range(5) if c[side] & (1 << i)]
            retained = selected[:c['budget']-3]
            retained_inputs.append(set(retained))
            cut = value[side]
            ids = {g['id'] for g in cut['grains']}
            assert ids == required | set(retained)
            assert cut['grain_count'] == len(ids) <= c['budget']
            assert cut['bond_count'] == len(retained)
            assert bool(cut['frontier']) == (len(selected) > len(retained))
            assert cut['receipts'] and cut['receipt_hash'] and cut['cut_hash']
        a, b = retained_inputs
        possible = a & b if c['operation'] == 'meet' else a | b
        cut = value['result']
        ids = {g['id'] for g in cut['grains']}
        optional = ids-required
        assert required <= ids and optional <= possible
        assert len(optional) == min(len(possible), c['budget']-3)
        assert cut['grain_count'] == len(ids) <= c['budget']
        assert cut['bond_count'] == len(optional)
        assert bool(cut['frontier']) == (len(possible) > len(optional))
        assert cut['receipts'] and cut['receipt_hash'] and cut['cut_hash']
    assert successful == 12288 and budget_failures == 4096 and invalid == 14
    return dict(successful=successful, required_budget_failures=budget_failures, typed_invalid=invalid)
