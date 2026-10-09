#!/usr/bin/env python3
"""Boil WordNet down to the thesaurus the backend serves.

    build_thesaurus.py WORDNET_DICT_DIR OUT_FILE

WORDNET_DICT_DIR is the `dict/` folder of a WordNet 3.x database (the
`wordnet-db` npm package has one: `npm pack wordnet-db@3.1.14`). OUT_FILE is
written gzipped — `backend/assets/thesaurus.tsv.gz` is the one the backend
embeds.

One line per word, in sense order, most common sense first:

    word<TAB>sense<TAB>sense...

and each sense is

    pos;gloss;synonym|synonym;similar|similar;antonym|antonym

with `pos` one of n, v, a, r. Senses with nothing to offer — no synonym, no
similar word, no antonym — are left out, and so are words left with none.
Multi-word entries keep their spaces ("look into").

WordNet 3.0 Copyright 2006 by Princeton University. All rights reserved.
Its licence travels with the output: see backend/assets/WORDNET_LICENSE.
"""
import gzip
import os
import sys

POS = {"noun": "n", "verb": "v", "adj": "a", "adv": "r"}
# Synset type letters in data files; satellites are adjectives.
SS_POS = {"n": "n", "v": "v", "a": "a", "s": "a", "r": "r"}
GLOSS_MAX = 70


def clean(lemma: str) -> str:
    # Adjective markers like "(a)" or "(p)", and underscores for spaces.
    if lemma.endswith(")") and "(" in lemma:
        lemma = lemma[: lemma.index("(")]
    return lemma.replace("_", " ")


def short_gloss(gloss: str) -> str:
    # The definition, without the quoted examples after it.
    text = gloss.split(";")[0].split('"')[0].strip().rstrip(";").strip()
    if len(text) > GLOSS_MAX:
        text = text[: GLOSS_MAX - 1].rsplit(" ", 1)[0] + "…"
    # The format's own separators can't appear inside a field.
    return text.replace("\t", " ").replace(";", ",").replace("|", "/")


def read_data(path: str, pos: str):
    """offset -> (lemmas, gloss, pointers)"""
    out = {}
    with open(path, encoding="latin-1") as f:
        for line in f:
            if line.startswith("  "):
                continue
            head, _, gloss = line.partition(" | ")
            parts = head.split()
            offset, ss_type = parts[0], parts[2]
            w_cnt = int(parts[3], 16)
            lemmas = [clean(parts[4 + 2 * i]) for i in range(w_cnt)]
            i = 4 + 2 * w_cnt
            p_cnt = int(parts[i])
            i += 1
            pointers = []
            for _ in range(p_cnt):
                sym, target, tpos, srctgt = parts[i : i + 4]
                pointers.append((sym, target, SS_POS[tpos], int(srctgt[:2], 16), int(srctgt[2:], 16)))
                i += 4
            out[(pos, offset)] = (lemmas, short_gloss(gloss), pointers, SS_POS[ss_type])
    return out


def read_counts(path: str):
    """(pos, offset) -> how often its words were seen in WordNet's tagged
    corpus. Used to put common words first: "stroll" before "lollop"."""
    counts = {}
    lemmas = {}
    sense_pos = {"1": "n", "2": "v", "3": "a", "4": "r", "5": "a"}
    with open(path, encoding="latin-1") as f:
        for line in f:
            key, offset, _, tag_cnt = line.split()
            pos = sense_pos[key.split("%")[1][0]]
            counts[(pos, offset)] = counts.get((pos, offset), 0) + int(tag_cnt)
            lemma = clean(key.split("%")[0])
            lemmas[lemma] = lemmas.get(lemma, 0) + int(tag_cnt)
    return counts, lemmas


def main():
    src, dest = sys.argv[1], sys.argv[2]
    synsets = {}
    for name, pos in POS.items():
        synsets.update(read_data(os.path.join(src, f"data.{name}"), pos))
    counts, common = read_counts(os.path.join(src, "index.sense"))
    # Most used first: "brilliant" before "fulgid". Stable, so WordNet's own
    # order breaks ties.
    by_use = lambda words: sorted(words, key=lambda w: -common.get(w.lower(), 0))

    # word -> [(pos, offset)] in WordNet's sense order.
    order = []
    senses = {}
    for name, pos in POS.items():
        with open(os.path.join(src, f"index.{name}"), encoding="latin-1") as f:
            for line in f:
                if line.startswith("  "):
                    continue
                parts = line.split()
                word = clean(parts[0])
                synset_cnt = int(parts[2])
                offsets = parts[-synset_cnt:]
                if word not in senses:
                    senses[word] = []
                    order.append(word)
                senses[word].extend((pos, o) for o in offsets)

    lines = []
    for word in sorted(order):
        out = []
        for key in senses[word]:
            lemmas, gloss, pointers, _ = synsets[key]
            synonyms = [l for l in lemmas if l.lower() != word.lower()]
            # (how common, word), sorted once all are in.
            similar, antonyms = [], []
            # Where in the synset this word sits, for word-to-word pointers.
            mine = {i + 1 for i, l in enumerate(lemmas) if l.lower() == word.lower()}
            for sym, target, tpos, src_n, tgt_n in pointers:
                other = synsets.get((tpos, target))
                if other is None:
                    continue
                if sym == "!" and (src_n == 0 or src_n in mine):
                    antonyms.append(other[0][tgt_n - 1] if tgt_n else other[0][0])
                elif sym in ("&", "^") and src_n == 0:
                    similar.extend((common.get(w.lower(), 0), w) for w in other[0][:3])
                elif sym == "~" and src_n == 0 and key[0] in ("n", "v"):
                    # The more particular ways of doing or being it: walk ->
                    # march, pace, shuffle.
                    best = by_use(other[0])[0]
                    similar.append((counts.get((tpos, target), 0) + common.get(best.lower(), 0), best))
            similar = [w for _, w in sorted(similar, key=lambda cw: -cw[0])]
            dedupe = lambda xs: list(dict.fromkeys(x for x in xs if x.lower() != word.lower()))
            synonyms, similar, antonyms = dedupe(synonyms), dedupe(similar), dedupe(antonyms)
            synonyms = by_use(synonyms)
            similar = [s for s in similar if s not in synonyms][:12]
            if not (synonyms or similar or antonyms):
                continue
            out.append(f"{key[0]};{gloss};{'|'.join(synonyms)};{'|'.join(similar)};{'|'.join(antonyms)}")
        if out:
            lines.append(word + "\t" + "\t".join(out))

    with gzip.open(dest, "wt", encoding="utf-8", compresslevel=9) as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(lines)} words -> {dest} ({os.path.getsize(dest) // 1024} KiB)")


if __name__ == "__main__":
    main()
