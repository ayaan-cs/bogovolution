# The 48 task more-itertools set: 24 train, 24 held out.
# Sourced by run-oss48.sh and replay.sh. The first 8 of each list are the
# 16 tasks of v2.3.0, with the same splits. The 32 new ones are spread by
# difficulty: 12 short (one loop or one itertools call), 14 medium (edge
# cases, generators, keyword arguments), 6 hard (index math, numeric code,
# several arguments that interact).
OSS48_TRAIN=take,tail,nth,flatten,partition,unique_justseen,prepend,quantify,dotproduct,iter_except,ilen,duplicates_justseen,exactly_n,polynomial_derivative,grouper,subslices,sliding_window,split_at,distribute,consecutive_groups,stagger,nth_combination,sort_together,polynomial_from_roots
OSS48_HELD_OUT=consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true,repeatfunc,triplewise,mark_ends,longest_common_prefix,interleave_longest,circular_shifts,before_and_after,transpose,intersperse,windowed,chunked,locate,zip_offset,reshape,chunked_even,totient
