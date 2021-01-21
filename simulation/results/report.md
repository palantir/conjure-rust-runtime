
# Report
<!-- Run `cargo test -p simulation --release` to regenerate this report. -->

```
                   all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=74%	client_mean=5.898039864s	server_cpu=1202s	client_received=2000/2000	server_resps=2000	codes={200=1480, 500=520}
                       all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=73.7%	client_mean=4.316942294s	server_cpu=1202s	client_received=2000/2000	server_resps=2000	codes={200=1473, 500=527}
                                 all_nodes_500[UNLIMITED_ROUND_ROBIN]:	success=50%	client_mean=601ms	server_cpu=1202s	client_received=2000/2000	server_resps=2000	codes={200=1000, 500=1000}
                      black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=44.8%	client_mean=601.677094ms	server_cpu=537.895s	client_received=895/2000	server_resps=895	codes={200=895}
                          black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=91.3%	client_mean=601ms	server_cpu=1097.426s	client_received=1826/2000	server_resps=1826	codes={200=1826}
                                    black_hole[UNLIMITED_ROUND_ROBIN]:	success=91.3%	client_mean=601ms	server_cpu=1097.426s	client_received=1826/2000	server_resps=1826	codes={200=1826}
                drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=8.2423105s	server_cpu=6770.04s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                    drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=267.16525ms	server_cpu=1068.661s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                              drastic_slowdown[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=267.16525ms	server_cpu=1068.661s	client_received=4000/4000	server_resps=4000	codes={200=4000}
           fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=64.2%	client_mean=121ms	server_cpu=511.1s	client_received=6000/6000	server_resps=6000	codes={200=3851, 400=2149}
               fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=93.5%	client_mean=121ms	server_cpu=686.8s	client_received=6000/6000	server_resps=6000	codes={200=5608, 400=392}
                         fast_400s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=93.5%	client_mean=121ms	server_cpu=686.8s	client_received=6000/6000	server_resps=6000	codes={200=5608, 400=392}
           fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=121.056311ms	server_cpu=5445.121s	client_received=45000/45000	server_resps=45011	codes={200=45000}
               fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=121.255711ms	server_cpu=5445.44s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                         fast_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=121.255711ms	server_cpu=5445.44s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                   one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=3.544863s	server_cpu=151s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                       one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=1.818776s	server_cpu=151s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                                 one_big_spike[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=950.83ms	server_cpu=375.537s	client_received=1000/1000	server_resps=2487	codes={200=1000}
one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=64.6%	client_mean=604.813737ms	server_cpu=1502.5s	client_received=2500/2500	server_resps=2500	codes={200=1616, 500=884}
    one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=84.7%	client_mean=601ms	server_cpu=1502.5s	client_received=2500/2500	server_resps=2500	codes={200=2117, 500=383}
              one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN]:	success=65.7%	client_mean=601ms	server_cpu=1502.5s	client_received=2500/2500	server_resps=2500	codes={200=1642, 500=858}
         server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.7%	client_mean=14748.691741795s	server_cpu=43824419.121s	client_received=150000/150000	server_resps=219121	codes={200=149618, 429=382}
             server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.7%	client_mean=0ns	server_cpu=43465417.326s	client_received=150000/150000	server_resps=217326	codes={200=149547, 429=453}
                       server_side_rate_limits[UNLIMITED_ROUND_ROBIN]:	success=99.1%	client_mean=318.216570307s	server_cpu=48633643.167s	client_received=150000/150000	server_resps=243167	codes={200=148625, 429=1375}
        short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.5%	client_mean=2.001s	server_cpu=3185.6s	client_received=1600/1600	server_resps=1600	codes={200=1592, 500=8}
            short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.3%	client_mean=2.001s	server_cpu=3177.6s	client_received=1600/1600	server_resps=1600	codes={200=1588, 500=12}
                      short_outage_on_one_node[UNLIMITED_ROUND_ROBIN]:	success=99.3%	client_mean=2.001s	server_cpu=3177.6s	client_received=1600/1600	server_resps=1600	codes={200=1588, 500=12}
          simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=791.227272ms	server_cpu=10444.2s	client_received=13200/13200	server_resps=13200	codes={200=13200}
              simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=788.257575ms	server_cpu=10405s	client_received=13200/13200	server_resps=13200	codes={200=13200}
                        simplest_possible_case[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=788.257575ms	server_cpu=10405s	client_received=13200/13200	server_resps=13200	codes={200=13200}
           slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=222.258ms	server_cpu=312.88s	client_received=1500/1500	server_resps=1584	codes={200=1500}
               slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=85.636666ms	server_cpu=123.356s	client_received=1500/1500	server_resps=1519	codes={200=1500}
                         slow_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=85.636666ms	server_cpu=123.356s	client_received=1500/1500	server_resps=1519	codes={200=1500}
   slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=203.8855747s	server_cpu=37183.813s	client_received=10000/10000	server_resps=13028	codes={200=10000}
       slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=176.1370036s	server_cpu=38481.382s	client_received=10000/10000	server_resps=13677	codes={200=9999, 500=1}
                 slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN]:	success=3.9%	client_mean=9.889748704s	server_cpu=195891.099s	client_received=10000/10000	server_resps=49086	codes={200=386, 500=9614}
                 uncommon_flakes[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=98.9%	client_mean=1ms	server_cpu=10s	client_received=10000/10000	server_resps=10000	codes={200=9887, 500=113}
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

            