
# Report
<!-- Run `cargo test -p simulation --release` to regenerate this report. -->

```
                   all_nodes_500[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=74.2%	client_mean=5.77465273s	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1483, 500=517}
                       all_nodes_500[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=73.7%	client_mean=4.30637042s	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1474, 500=526}
                                 all_nodes_500[UNLIMITED_ROUND_ROBIN]:	success=50%	client_mean=600ms	server_cpu=1200s	client_received=2000/2000	server_resps=2000	codes={200=1000, 500=1000}
                      black_hole[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=44.8%	client_mean=600.653631ms	server_cpu=537s	client_received=895/2000	server_resps=895	codes={200=895}
                          black_hole[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=91.4%	client_mean=600ms	server_cpu=1096.2s	client_received=1827/2000	server_resps=1827	codes={200=1827}
                                    black_hole[UNLIMITED_ROUND_ROBIN]:	success=91.4%	client_mean=600ms	server_cpu=1096.2s	client_received=1827/2000	server_resps=1827	codes={200=1827}
                drastic_slowdown[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=8.22676525s	server_cpu=6765.659s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                    drastic_slowdown[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=260.8455ms	server_cpu=1043.382s	client_received=4000/4000	server_resps=4000	codes={200=4000}
                              drastic_slowdown[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=260.8455ms	server_cpu=1043.382s	client_received=4000/4000	server_resps=4000	codes={200=4000}
           fast_400s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=64.2%	client_mean=120ms	server_cpu=505.1s	client_received=6000/6000	server_resps=6000	codes={200=3851, 400=2149}
               fast_400s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=93.6%	client_mean=120ms	server_cpu=681.7s	client_received=6000/6000	server_resps=6000	codes={200=5617, 400=383}
                         fast_400s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=93.6%	client_mean=120ms	server_cpu=681.7s	client_received=6000/6000	server_resps=6000	codes={200=5617, 400=383}
           fast_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=120.052266ms	server_cpu=5400.1s	client_received=45000/45000	server_resps=45010	codes={200=45000}
               fast_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=120.254822ms	server_cpu=5400.4s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                         fast_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=120.254822ms	server_cpu=5400.4s	client_received=45000/45000	server_resps=45040	codes={200=45000}
                   one_big_spike[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=3.427156s	server_cpu=150s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                       one_big_spike[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=1.796529s	server_cpu=150s	client_received=1000/1000	server_resps=1000	codes={200=1000}
                                 one_big_spike[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=952.571ms	server_cpu=373.2s	client_received=1000/1000	server_resps=2488	codes={200=1000}
one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=65%	client_mean=603.724307ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=1625, 500=875}
    one_endpoint_dies_on_each_server[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=84.6%	client_mean=600ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=2116, 500=384}
              one_endpoint_dies_on_each_server[UNLIMITED_ROUND_ROBIN]:	success=65.7%	client_mean=600ms	server_cpu=1500s	client_received=2500/2500	server_resps=2500	codes={200=1642, 500=858}
         server_side_rate_limits[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.7%	client_mean=0ns	server_cpu=43810600s	client_received=150000/150000	server_resps=219053	codes={200=149520, 429=480}
             server_side_rate_limits[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.8%	client_mean=0ns	server_cpu=43152200s	client_received=150000/150000	server_resps=215761	codes={200=149633, 429=367}
                       server_side_rate_limits[UNLIMITED_ROUND_ROBIN]:	success=99%	client_mean=314.908823132s	server_cpu=48239400s	client_received=150000/150000	server_resps=241197	codes={200=148484, 429=1516}
        short_outage_on_one_node[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=99.5%	client_mean=2s	server_cpu=3184.008s	client_received=1600/1600	server_resps=1600	codes={200=1592, 500=8}
            short_outage_on_one_node[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=99.3%	client_mean=2s	server_cpu=3176.012s	client_received=1600/1600	server_resps=1600	codes={200=1588, 500=12}
                      short_outage_on_one_node[UNLIMITED_ROUND_ROBIN]:	success=99.3%	client_mean=2s	server_cpu=3176.012s	client_received=1600/1600	server_resps=1600	codes={200=1588, 500=12}
          simplest_possible_case[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=790.227272ms	server_cpu=10431s	client_received=13200/13200	server_resps=13200	codes={200=13200}
              simplest_possible_case[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=790.121212ms	server_cpu=10429.6s	client_received=13200/13200	server_resps=13200	codes={200=13200}
                        simplest_possible_case[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=790.121212ms	server_cpu=10429.6s	client_received=13200/13200	server_resps=13200	codes={200=13200}
           slow_503s_then_revert[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=221.192666ms	server_cpu=311.283s	client_received=1500/1500	server_resps=1584	codes={200=1500}
               slow_503s_then_revert[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=84.623333ms	server_cpu=121.836s	client_received=1500/1500	server_resps=1519	codes={200=1500}
                         slow_503s_then_revert[UNLIMITED_ROUND_ROBIN]:	success=100%	client_mean=84.623333ms	server_cpu=121.836s	client_received=1500/1500	server_resps=1519	codes={200=1500}
   slowdown_and_error_thresholds[CONCURRENCY_LIMITER_PIN_UNTIL_ERROR]:	success=100%	client_mean=202.074443s	server_cpu=38021.992s	client_received=10000/10000	server_resps=12948	codes={200=10000}
       slowdown_and_error_thresholds[CONCURRENCY_LIMITER_ROUND_ROBIN]:	success=100%	client_mean=175.846126012s	server_cpu=38521.201s	client_received=10000/10000	server_resps=13677	codes={200=9999, 500=1}
                 slowdown_and_error_thresholds[UNLIMITED_ROUND_ROBIN]:	success=3.9%	client_mean=9.988361538s	server_cpu=195750.319s	client_received=10000/10000	server_resps=49079	codes={200=390, 500=9610}
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

            