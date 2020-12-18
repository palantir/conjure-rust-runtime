
# Report
<!-- Run `cargo test -p simulation --release` to regenerate this report. -->

```
                   all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=74.2%	client_mean=6.093870532s	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1483, 500=517}
                       all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=73.7%	client_mean=4.80412763s	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1473, 500=527}
                                 all_nodes_500[UNLIMITED_ROUND_ROBIN]:	success=50%	client_mean=600ms	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1000, 500=1000}
                      black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=30%	client_mean=600.6ms	server_cpu=360s	client_received=600/2000	server_resps=600	codes={200=600}
                          black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=91.4%	client_mean=600ms	server_cpu=1096.8s	client_received=1828/2000	server_resps=1828	codes={200=1828}
                                    black_hole[UNLIMITED_ROUND_ROBIN]:	success=91.4%	client_mean=600ms	server_cpu=1096.8s	client_received=1828/2000	server_resps=1828	codes={200=1828}
                drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=11.357176s	server_cpu=8927.395s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                    drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=260.7595ms	server_cpu=1043.038s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                              drastic_slowdown[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=260.7595ms	server_cpu=1043.038s	client_received=4000/4000	server_resps=4000	codes={200=4000}
           fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=55%	client_mean=120ms	server_cpu=450s	client_received=6000/6000	server_resps=6000	codes={200=3300, 400=2700}
               fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=93.6%	client_mean=120ms	server_cpu=681.7s	client_received=6000/6000	server_resps=6000	codes={200=5617, 400=383}
                         fast_400s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=93.6%	client_mean=120ms	server_cpu=681.7s	client_received=6000/6000	server_resps=6000	codes={200=5617, 400=383}
           fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=120.0654ms	server_cpu=5400.14s	client_received=45000/45000	server_resps=45014	codes={200=45000}
               fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=120.160222ms	server_cpu=5400.4s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                         fast_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=120.160222ms	server_cpu=5400.4s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                   one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=3.448671s	server_cpu=150s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                       one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=1.799832s	server_cpu=150s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                                 one_big_spike[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=928.784ms	server_cpu=369.005s	client_received=1000/1000	server_resps=2460	codes={200=1000}
one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=64.4%	client_mean=603.734326ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=1611, 500=889}
    one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=84.8%	client_mean=600.117036ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=2119, 500=381}
              one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN]:	success=65.2%	client_mean=600ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=1631, 500=869}
         server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.7%	client_mean=9494.755138122s	server_cpu=44363600s	client_received=150000/150000	server_resps=221818	codes={200=149584, 429=416}
             server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.7%	client_mean=0ns	server_cpu=43180600s	client_received=150000/150000	server_resps=215903	codes={200=149611, 429=389}
                       server_side_rate_limits[UNLIMITED_ROUND_ROBIN]:	success=99%	client_mean=314.906035979s	server_cpu=48245000s	client_received=150000/150000	server_resps=241225	codes={200=148475, 429=1525}
        short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.4%	client_mean=2s	server_cpu=3180.01s	client_received=1600/1600	server_resps=1600	codes={200=1590, 500=10}
            short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.2%	client_mean=2s	server_cpu=3174.013s	client_received=1600/1600	server_resps=1600	codes={200=1587, 500=13}
                      short_outage_on_one_node[UNLIMITED_ROUND_ROBIN]:	success=99.2%	client_mean=2s	server_cpu=3174.013s	client_received=1600/1600	server_resps=1600	codes={200=1587, 500=13}
          simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=799.893939ms	server_cpu=10558.6s	client_received=13200/13200	server_resps=13200	codes={200=13200}
              simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=787.10606ms	server_cpu=10389.8s	client_received=13200/13200	server_resps=13200	codes={200=13200}
                        simplest_possible_case[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=787.10606ms	server_cpu=10389.8s	client_received=13200/13200	server_resps=13200	codes={200=13200}
           slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=370.087333ms	server_cpu=499.902s	client_received=1500/1500	server_resps=1623	codes={200=1500}
               slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=85.196ms	server_cpu=124.026s	client_received=1500/1500	server_resps=1521	codes={200=1500}
                         slow_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=85.196ms	server_cpu=124.026s	client_received=1500/1500	server_resps=1521	codes={200=1500}
   slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=86.6%	client_mean=219.611418379s	server_cpu=33867.441s	client_received=8662/10000	server_resps=11507	codes={200=8662}
       slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=175.8007066s	server_cpu=38513.01s	client_received=10000/10000	server_resps=13649	codes={200=10000}
                 slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN]:	success=3.7%	client_mean=8.905163487s	server_cpu=195890.766s	client_received=10000/10000	server_resps=49079	codes={200=367, 500=9633}
                 uncommon_flakes[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=98.9%	client_mean=1ms	server_cpu=10s	client_received=10000/10000	server_resps=10000	codes={200=9889, 500=111}
                     uncommon_flakes[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=98.9%	client_mean=1ms	server_cpu=10s	client_received=10000/10000	server_resps=10000	codes={200=9886, 500=114}
                               uncommon_flakes[UNLIMITED_ROUND_ROBIN]:	success=98.9%	client_mean=1ms	server_cpu=10s	client_received=10000/10000	server_resps=10000	codes={200=9886, 500=114}
```


## `all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `all_nodes_500[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/all_nodes_500[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="all_nodes_500[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `black_hole[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/black_hole[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="black_hole[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `drastic_slowdown[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/drastic_slowdown[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="drastic_slowdown[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `fast_400s_then_revert[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_400s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="fast_400s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `fast_503s_then_revert[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/fast_503s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="fast_503s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `one_big_spike[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_big_spike[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="one_big_spike[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `server_side_rate_limits[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/server_side_rate_limits[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="server_side_rate_limits[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `short_outage_on_one_node[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/short_outage_on_one_node[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="short_outage_on_one_node[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `simplest_possible_case[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/simplest_possible_case[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="simplest_possible_case[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `slow_503s_then_revert[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slow_503s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="slow_503s_then_revert[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `uncommon_flakes[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/uncommon_flakes[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
        <td><image width=400 src="uncommon_flakes[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR].png" /></td>
    </tr>
</table>

## `uncommon_flakes[CONCURRENCY_LIMITER_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/uncommon_flakes[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="uncommon_flakes[CONCURRENCY_LIMITER_ROUND_ROBIN].png" /></td>
    </tr>
</table>

## `uncommon_flakes[UNLIMITED_ROUND_ROBIN]`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src="https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/master/simulation/results/uncommon_flakes[UNLIMITED_ROUND_ROBIN].png" /></td>
        <td><image width=400 src="uncommon_flakes[UNLIMITED_ROUND_ROBIN].png" /></td>
    </tr>
</table>

            