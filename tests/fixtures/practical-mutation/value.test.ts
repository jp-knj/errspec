import {test, expect} from 'vitest';
import {accepts, firstIndex, reference} from './src/value';

test('the exact boundary is allowed', () => expect(accepts('x'.repeat(8))).toBe(true));
test('a unique reference keeps its index', () => expect(firstIndex([{kind:'notice',ref:'a'},{kind:'notice',ref:'b'}], 'b')).toBe(1));
test('an ordinary object supplies a reference', () => expect(reference({ref:'a'})).toBe('a'));
