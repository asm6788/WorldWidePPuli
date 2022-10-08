use http::{HeaderMap, StatusCode};
use indicatif::ProgressBar;
use linecount::count_lines;
use petgraph::Graph;
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{hash_map::Entry, HashMap},
    fs::File,
};
use urlencoding::decode;
use warp::Filter;

#[tokio::main]
async fn main() {
    let path = "test.csv";
    let pb = ProgressBar::new(count_lines(File::open(path.to_string()).unwrap()).unwrap() as u64);
    let mut rdr = csv::Reader::from_path(path.to_string()).unwrap();
    let record = rdr.records();

    let mut graph = Graph::<String, u32>::new();
    let mut node_map = HashMap::new();

    for result in record {
        pb.inc(1);
        match result {
            Ok(result) => {
                let origin = *match node_map.entry(result[0].to_string()) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(graph.add_node(result[0].to_string())),
                };

                let dest = *match node_map.entry(result[1].to_string()) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(graph.add_node(result[1].to_string())),
                };

                graph.add_edge(origin, dest, result[2].parse::<u32>().unwrap());
            }
            Err(_) => {
                continue;
            }
        }
    }
    pb.finish();

    let cors = warp::cors().allow_any_origin();

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "text/html; charset=utf-8".parse().unwrap());

    let frontend = warp::path::end()
        .and(warp::fs::file("frontend/index.html"))
        .with(warp::reply::with::headers(headers.clone()));

    let search = warp::path!("neighbors" / String)
        .and(warp::query::<Query>())
        .map(move |parents: String, option: Query| {
            let parents = decode(&parents).unwrap().into_owned();

            let mut stopword: Vec<String> =
                option.stopword.split(",").map(|s| s.to_string()).collect();

            let mut stopword_preset = HashMap::new();
            stopword_preset.insert(
                "[나라]",
                vec![
                    "대한민국",
                    "미국",
                    "영국",
                    "프랑스",
                    "독일",
                    "이탈리아",
                    "중국",
                    "러시아",
                    "일본",
                    "북한",
                    "소련",
                ],
            );
            stopword_preset.insert(
                "[대한민국 대통령]",
                vec![
                    "이승만",
                    "윤보선",
                    "박정희",
                    "최규하",
                    "전두환",
                    "노태우",
                    "김영삼",
                    "김대중",
                    "노무현",
                    "이명박",
                    "박근혜",
                    "문재인",
                    "윤석열",
                ],
            );
            stopword_preset.insert(
                "[해외 정치인]",
                vec![
                    "조 바이든",
                    "도널드 트럼프",
                    "버락 오바마",
                    "기시다 후미오",
                    "스가 요시히데",
                    "아베 신조",
                    "올라프 숄츠",
                    "앙겔라 메르켈",
                    "리즈 트러스",
                    "보리스 존슨",
                    "테레사 메이",
                    "에마뉘엘 마크롱",
                    "프랑수아 올랑드",
                    "볼로디미르 젤렌스키",
                    "블라디미르 푸틴",
                    "시진핑",
                    "후진타오",
                    "김일성",
                    "김정일",
                    "김정은",
                ],
            );

            for (key, value) in stopword_preset {
                if stopword.contains(&key.to_string()) {
                    stopword.remove(stopword.iter().position(|x| x == &key).unwrap());
                    stopword.extend(value.iter().map(|s| s.to_string()));
                }
            }

            match node_map.get(&parents) {
                Some(a) => {
                    let mut result = Graph::<String, u32>::new();
                    let mut map = HashMap::new();
                    search_neighbors(
                        &graph,
                        *a,
                        0,
                        option.depth,
                        &stopword,
                        &mut map,
                        &mut result,
                    );

                    let mut nodes = Vec::new();
                    for node in result.node_indices().collect::<Vec<_>>() {
                        nodes.push(Node {
                            id: node.index(),
                            name: result[node].to_string(),
                        });
                    }

                    let mut links = Vec::new();
                    for edge in result.edge_indices().collect::<Vec<_>>() {
                        links.push(Link {
                            source: result.edge_endpoints(edge).unwrap().0.index(),
                            target: result.edge_endpoints(edge).unwrap().1.index(),
                            value: *result.edge_weight(edge).unwrap(),
                        });
                    }

                    return http::Response::builder()
                        .header("content-type", "application/json")
                        .body(
                            serde_json::to_string(&json!({
                                "nodes": nodes,
                                "links": links
                            }))
                            .unwrap(),
                        )
                        .unwrap();
                }
                None => {
                    return http::Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body("Not Found".into())
                        .unwrap()
                }
            }
        })
        .or(frontend)
        .with(cors);

    warp::serve(search).run(([0, 0, 0, 0], 3030)).await;
}

fn search_neighbors(
    graph: &Graph<String, u32>,
    atarget: petgraph::graph::NodeIndex,
    depth: u8,
    max_depth: u8,
    stopword: &Vec<String>,
    map: &mut HashMap<String, petgraph::graph::NodeIndex>,
    result: &mut Graph<String, u32>,
) {
    if depth >= max_depth {
        return;
    }

    if stopword.contains(&graph[atarget]) {
        return;
    }

    for i in 0..2 {
        let mut neighbors = graph
            .neighbors_directed(
                atarget,
                if i == 0 {
                    petgraph::Direction::Outgoing
                } else {
                    petgraph::Direction::Incoming
                },
            )
            .detach();

        while let Some((edge, target)) = neighbors.next(&graph) {
            if atarget != target {
                //dot 출력
                let origin = *match map.entry(graph[atarget].to_string()) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(result.add_node(graph[atarget].to_string())),
                };

                let dest = *match map.entry(graph[target].to_string()) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(result.add_node(graph[target].to_string())),
                };

                if i == 0 {
                    result.add_edge(origin, dest, graph[edge]);
                } else {
                    result.add_edge(dest, origin, graph[edge]);
                }

                search_neighbors(graph, target, depth + 1, max_depth, stopword, map, result);
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Query {
    stopword: String,
    depth: u8,
}

#[derive(Serialize, Deserialize)]
struct Node {
    id: usize,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct Link {
    source: usize,
    target: usize,
    value: u32,
}
