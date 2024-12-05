#let title = [Engenharia de Serviços em Rede]
#let authors = (
  (name: "Daniel Pereira", affiliation: "PG55928"),
  (name: "Francisco Ferreira", affiliation: "PG55942"),
  (name: "Rui Lopes", affiliation: "PG56009"),
)
#set document(author: authors.map(a => a.name), title: title)
#set page(numbering: "1", number-align: center)
#set text(font: "IBM Plex Sans", lang: "pt", region: "PT", size: 10pt)

#set par(justify: true)

#show link: set text(fill: blue.darken(30%))
#show link: underline

#align(center)[
  #block(text(weight: 700, 1.75em, title))
  #block(text(weight: 500, 1.2em, [TP2]))
]

// Authors
#pad(
  top: 0.5em,
  bottom: 0.5em,
  x: 2em,
  grid(
    columns: (1fr,) * calc.min(3, authors.len()),
    gutter: 1em,
    ..authors.map(author => align(center)[
      *#author.name* \
      #author.affiliation
    ]),
  ),
)

#let todo = text.with(fill: red)

#let packet(title, ..body) = {
  set raw(lang: "rust")
  table(
    columns: (1fr, 1fr, 2fr),
    inset: 5pt,
    align: horizon,
    table.header(table.cell(colspan: 3, fill: rgb("#D9D9D9"), text(weight: "bold", title))),

    [*Campo*], [*Tipo*], [*Descrição*],
    ..body
  )
}


= Introdução
Este relatório tem como objetivo apresentar o trabalho prático desenvolvido durante a unidade curricular de Engenharia de Serviços em Rede. O trabalho consistiu no desenvolvimento de um serviço _over the top_ para a entrega de conteúdos multimédia em tempo real. O relatório terá como objetivo apresentar a arquitetura do sistema, a sua descrição, implementação e as decisões tomadas durante o desenvolvimento do mesmo.

Uma das liberdades dadas pela equipa docente foi a da escolha da linguagem de programação para o desenvolvimento do projeto. O nosso grupo decidiu optar por Rust. Esta escolha permitiu-nos desenvolver um sistema muito performante e seguro sem grandes esforços.

= Arquitetura
Tal como mencionado anteriormente, neste trabalho pretende-se a criação de um sistema de entrega de conteúdos multimédia em tempo real, a partir de um, ou mais, servidores de _streaming_ para um dado conjunto de clientes. Este sistema deverá estar assente em cima de uma rede _overlay_ própria.

Para isso, um *cliente* deverá escolher um *_point of presence_* #footnote[Essa escolha e a forma como é feita será abordada noutro capítulo.] como ponto de acesso à rede de entrega. Este _point of presence_ estará, presumivelmente, a receber conteúdos por parte da dita rede. Sendo os mesmos conteúdos enviados, por estes _point of presence_, via _unicast_ para os clientes interessados.

A chave para um bom funcionamento do sistema como um todo está na monitorização e manutenção da rede de entrega, composta por *_nodes_*. Estes, devem formar, entre si, uma árvore de distribuição ótima #footnote[Baseando-se sempre num conjunto de métricas bem definidas.] para a devida entrega dos conteúdos. Os *_nodes_* e os *point of presence* rodam a mesma aplicação, sem diferenças, só que o *point of presence* está acessível, na rede, pelos clientes.

O fluxo das _streams_, propriamente ditas, está ao encargo do (ou dos) *servidor*. Uma vez que se pretende um serviço em tempo real (e não _on demand_) parte-se do princípio de que o servidor estará sempre a enviar os _bytes_ codificados das _streams_ existentes e a propagar para a rede, quando necessário #footnote[Por necessário entenda-se existirem clientes interessados numa determinada _stream_.]. Estes _bytes_ são, então, depois enviados para a rede de entrega, navegando através da árvore de distribuição.

As possíveis ligações de cada um dos nodos é ditada a partir do *bootstrapper*. Este contém um ficheiro de configuração com a lista de vizinhos de todos os nodos. Quando iniciado este programa, abre um _socket_ onde os *nodes* e os *servidores* contactam para se informarem dos seus vizinhos. O *bootstrapper* só serve para a inicialização dos nodos/servidor e não é mais usado após isso.

De seguida, apresentamos um diagrama arquitetural genérico que tem como objetivo apresentar de que forma os diferentes componentes do sistema se dispõem e comunicam entre si.

#figure(
  caption: [Diagrama arquitetural do sistema],
  image("assets/arquitetura.png"),
)

É possível observar que o protocolo de transporte utilizado em praticamente todo o sistema é o UDP. Esta escolha baseia-se, principalmente, na performance que um protocolo como estes nos poderá dar, se em comparação com o TCP.

De facto, dentro da rede de entrega, a utilização de um protocolo como o TCP não traria qualquer vantagem, a não ser para pacotes de controlo e informação de metadados trocadas entre os nodos. Para esses casos em que é necessário assegurar que os pacotes cheguem ao destino corretamente, existe então a noção de pacotes _reliable_, e o seu oposto, pacotes _unreliable_. Entraremos em mais detalhe numa secção abaixo.

= Construção da Topologia Overlay
Para a construção da topologia _overlay_ o grupo decidiu optar pela abordagem baseada num controlador, o *_bootstrapper_*.

Apesar desta abordagem "centralizada" para a entrada de novos _nodes_, a recuperação de falhas é feita de forma autónoma pela rede de entrega #footnote[Ou seja, é uma rede _self-healing_.] e não depende, de forma alguma, do *_bootstrapper_*, processo este que será abordado noutro capítulo.

Essencialmente, quando num dado nó da rede _underlay_ é instanciado um programa do tipo _node_ este passa a fazer parte da _overlay_. Esta entrada começa com o contacto, por parte do _node_, ao dito _bootstrapper_. Em resposta, o _bootstrapper_ envia uma lista com os vizinhos do dado _node_, caso este exista na topologia para a qual o _bootstrapper_ se encontra configurado no momento.

Uma vez que a comunicação realizada com o _bootstrapper_ é extremamente simples e baseia-se na troca de dois pacotes, o grupo optou por utilizar TCP.

== Árvore de Distribuição Ótima
Esta etapa é essencial para o bom funcionamento da rede de entrega. O objetivo aqui passa por encontrar a árvore que maximiza, em termos de algumas métricas #footnote[Estas métricas serão abordadas, com mais detalhe, noutro capítulo.], a performance da distribuição dos conteúdos na rede completa.

Para tal, o grupo decidiu utilizar um algoritmo baseado no célebre algoritmo de Prim. Dado um grafo pesado não direcionado, este algoritmo é capaz de encontrar a _minimum spanning tree_, que é um _subset_ do grafo que conecta todos os nós, sem ciclos, e com o menor peso total possível.

Após a topologia _overlay_ se encontrar num estado em que possa ser considerada usável, começa a etapa de construção da árvore de distribuição ótima. Esta etapa pode ser divida nos seguintes passos:

+ O servidor começa por enviar, para os seus vizinhos, um pacote, nomeado como _flood_;

+ Os vizinhos recebem o pacote e registam de quem o receberam (consideram estes _nodes_ como provedor de _streams_), atualizam as métricas existentes e propagam o pacote para os restantes vizinhos (exceto de quem o receberam);

+ O processo de _flooding_ repete-se até ao pacote não poder ser mais propagado.

Durante o passo 2., o _node_ que recebe o pacote atualiza, no seu estado interno, dados para calcular o melhor _node_ a quem solicitar _streams_ no futuro. Formando, assim, um dos ramos da dita árvore.

Para evitar que existam, de todo, ciclos aquando deste _flooding_, o pacote de _flood_ contém uma lista com os identificadores únicos dos _nodes_ por onde já passou #footnote[Este identificador único é dado pelo _*bootstrapper*_. Entrando em detalhes de implementação, este identificador é apenas a _hash_ do IP do _node_.]. Assim, caso um _node_ receba um pacote que contém o seu identificador, descarta-o.

Finalmente, é de notar que a árvore de distribuição ótima não se irá manter, indubitavelmente, constante durante o tempo de vida da _overlay_ #footnote[Isto pois, a rede está exposta a nuances externas que podem fazer variar fatores como a latência entre dois _nodes_.] . Assim, o grupo optou por criar um sistema de monitorização da rede capaz de alterar a árvore usada em _runtime_. Este sistema irá ser abordado no capítulo que lhe concerne.

= Bootstrapper
Este componente é muito simples, consistindo num programa cujo objetivo é receber conexões de _nodes_/servidores e responder imediatamente com uma lista dos respetivos vizinhos a partir do IP de contacto.

Assim, o pacote de resposta a tais conexões é o seguinte:

#figure(
  caption: [Pacote enviado do _bootstrapper_ para um _node_ que acabou de entrar na rede],
  packet(
    [BootstrapperNeighboursResponse],
    `neighbours`,
    `Vec<IpAddr>`,
    [Lista com os endereços IP dos vizinhos],
    `id`,
    `u64`,
    [Identificador único do _node_/servidor],
  ),
)

Os detalhes de serialização dos pacotes serão descritos numa secção abaixo.

O mapeamento de um _node_ para os seus vizinhos encontra-se num ficheiro de configuração TOML passado como argumento ao programa:

#figure(
  caption: [Exemplo de ficheiro de configuração do _bootstrapper_],
  ```toml
  # neighbours.toml
  "127.0.0.1" = ["127.0.0.2", "127.0.0.3"]
  "127.0.0.2" = ["127.0.0.1", "127.0.0.4"]
  "127.0.0.3" = ["127.0.0.1", "127.0.0.4"]
  "127.0.0.4" = ["127.0.0.2", "127.0.0.3"]
  ```,
)

Quando o _node_/servidor recebe a resposta, pode prosseguir na sua inicialização, onde também fecha conexão com o _bootstrapper_.

#pagebreak()

= Cliente
Este componente é a ponte entre o utilizador real e o resto do sistema. O mesmo é responsável por listar e reproduzir _streams_ disponíveis na rede.

O cliente recebe por parâmetro a lista de _point of presences_ que se pode conectar. Quando o programa do cliente inicia, conecta-se a esses _PoP_ e aguarda pela resposta da lista de _streams_ disponíveis, onde o cliente pode selecionar uma ou mais para visualizar.

Após a seleção de uma _stream_ a visualizar, o cliente contacta o melhor _point of presence_ para solicitar a _stream_. De realçar que é possível que um mesmo cliente visualize várias _streams_ ao mesmo tempo e enviadas por _point of presence_ diferentes.

== Escolha do Point of Presence
A escolha de qual _point of presence_ se ligar é baseada numa constante monitorização do estado da rede até aos diversos _point of presence_ #footnote[A lista de _point of presence_ é passada como argumento ao programa do cliente.], por iniciativa do cliente.

O _point of presence_ contactado aproveita para responder com a lista de vídeos disponíveis para consumo. Esta lista é enviada, desde o servidor, até aos _point of presence_ através do pacote de _flood_ mencionado anteriormente. Esta decisão permite que o sistema consiga lidar, facilmente, com a adição de _streams_ em _runtime_, que abordaremos mais tarde.

O _point of presence_ também envia o _timestamp_ de quando o pacote de _ping_ chegou, para possibilitar o cálculo bi-direcional do _ping_. A separação do tempo de ida e o tempo de volta só é usado para efeitos de visualização, visto que em todos os cálculos é usado o RTT.

A métrica aqui utilizada para a escolha do _point of presence_ é a média do _round trip time_ (RTT) num dado intervalo de tempo. Métrica que vai sendo calculada à medida que os clientes enviam _pings_ para os diversos _point of presence_.

Como estamos a usar UDP, perdas de pacotes podem acontecer. A forma como isso impacta o uso do RTT para a seleção do melhor _point of presence_ será explicada numa secção abaixo.

De seguida, encontra-se o pacote enviado por parte de um cliente para cada um dos _point of presence_ existentes.

#figure(
  caption: [Pacote enviado de um cliente para um _point of presence_ para monitorar a ligação],
  packet(
    [ClientPing],
    `sequence_number`,
    `u64`,
    [Utilizado para fazer _match_ entre o pedido e a resposta],
    `requested_videos`,
    `Vec<u8>`,
    [Lista de vídeos pedidos atuais do cliente],
  ),
)

E, agora, a resposta enviada pelo _point of presence_ contactado.

#figure(
  caption: [Pacote enviado de um _point of presence_ em resposta a um ClientPing],
  packet(
    [VideoList],
    `sequence_number`,
    `u64`,
    [Utilizado para fazer _match_ entre o pedido e a resposta],
    `videos`,
    `Vec<(u8, String)>`,
    [Lista de pares identificador da _stream_, nome da _stream_],
    `created_at`,
    `SystemTime`,
    [_Timestamp_ de criação do pacote, para a medição one-way do _ping_],
  ),
)

== Streaming do vídeo

Quando o cliente escolhe um vídeo para reproduzir, é enviado um pacote de pedido de vídeo para o _point of presence_. Esse _PoP_ é responsável por reencaminhar esse pacote para o melhor _node_ que ele conhece, recursivamente, até chegar ao servidor que contenha esse vídeo.

Quando o servidor recebe o pacote, começa a enviar os pacotes de vídeo referentes ao vídeo pedido pelo caminho escolhido, até chegar de volta ao cliente.

Agora, o cliente deve começar a receber os pacotes de vídeo, onde são reencaminhados para o _player_ iniciado. Este _player_ é, por omissão, o #link("https://mpv.io/")[MPV], mas também pode ser o `ffplay`, onde é possível alterá-lo passando-o por argumento do programa. Caso o utilizador não tenha o MPV ou o ffplay instalado no computador, é enviado uma mensagem de erro no começo do programa.

== Deteção de falhas

O cliente envia pacotes de ping para todos os _point of presence_ que conhece continuamente. Caso um _point of presence_ não responda aos pacotes de _ping_ passado 3 vezes o intervalo entre pacotes, esse _PoP_ é determinado como `Unresponsive`. Este intervalo de pacotes tem de valor de 1 segundo, valor este que foi idealizado para encontrar um meio termo entre não causar peso desnecessário na rede, mas ao mesmo tempo o cliente detetar rápido mortes de _PoPs_.

Quando um _PoP_ é determinado como `Unresponsive`, todas as _streams_ providenciadas por ele são reencaminhadas para o melhor _PoP_ atualmente. Como o _PoP_ anterior foi determinado como _Unresponsive_ este já não pode ser o melhor. Caso não haja _PoPs_ que consigam providenciar o vídeo, o vídeo é colocado numa fila à espera. Quando existir um _PoP_ que consiga reproduzir o vídeo (ou o _PoP_ morto voltar à vida), o cliente pede o vídeo a esse _PoP_ e continua a reprodução do vídeo.

== Interface Gráfica
Como forma de dar uma melhor experiência ao utilizador final, o grupo decidiu desenvolver uma interface gráfica. Esta interface demonstra a lista de _streams_ disponíveis para consumo, tal como opções para começar ou parar uma dita _stream_. Possui ainda informação relevante relacionada aos _point of presence_ conhecidos pelo cliente.

#figure(
  caption: [Interface gráfica presente no componente do cliente],
  image("assets/egui.png"),
)

#todo[Se calhar altera-se a nomenclatura disto de Servers para Points of Presence e tira-se outra print também com mais points of presence ligados]

A interface é responsável por criar uma instância de um _player_ capaz de mostrar a _stream_, quando clicado no botão "Play". Quando a _stream_ está a ser reproduzida, o utilizador pode simplesmente fechar o _player_ ou clicar no botão "Stop", que aparecerá no mesmo lugar do botão "Play".

#pagebreak()

= Node
Este é, muito provavelmente, o componente mais importante do sistema. O mesmo tem como objetivo integrar a rede de distribuição, reencaminhado os pacotes de _streaming_ necessários.

Essencialmente, este componente possui dois papéis: receber pedidos de _streams_ e devolver respostas com as respetivas _streams_. Fazendo, assim, com que a árvore de distribuição ótima construída seja corretamente utilizada.

= Point of Presence
Um _point of presence_ pode ser entendido como um _node_ que se encontra na fronteira entre os clientes e o resto da rede. São estes _nodes_ que são dados a conhecer (o seu IP) aos clientes quando estes querem consumir os conteúdos multimédia.

Portanto, toda a funcionalidade entre um _point of presence_ e um _node_ é exatamente igual, tratando-se, efetivamente, do mesmo programa.

Quando, por parte de um cliente, um dado _point of presence_ recebe um pedido de _stream_ existem dois cenários possíveis:

+ O mesmo já se encontra a enviar pacotes dessa _stream_ para outros clientes e, neste caso, cria um fluxo de dados e responde diretamente ao cliente solicitante;

+ O _point of presence_ não possui a _stream_ e, portanto, solicita ao seu melhor parente (ramo na árvore de distribuição ótima) que lhe envie a _stream_.

O processo em 2. ocorre recursivamente até que algures exista um _node_ que já possui a _stream_. No pior dos casos, este _node_ seria o servidor. Desta forma, todos os ramos da árvore, por onde o pedido passou, são ativados para a _stream_ que foi solicitada. Esta ativação faz com que, obviamente, seja desnecessário consultar a rede toda quando uma mesma _stream_ for, futuramente, solicitada por outros clientes.

#figure(
  caption: [Pacote enviado por parte de um cliente para iniciar uma _stream_],
  packet(
    "RequestVideo",
    [id],
    [u64],
    [Identificador da _stream_],
  ),
)

No caso em que um cliente deseja parar de visualizar a sua _stream_ existem, também, dois cenários possíveis:

1. O _point of presence_ ainda transmite a dita _stream_ para outros clientes e, no caso, apenas desativa o fluxo criado para o cliente que deseja parar;

2. Não existem outros clientes interessados na _stream_ e, portanto, é enviado um pedido de paragem para o seu parente.

Novamente, o processo em 2. ocorre recursivamente até que algures exista um _node_ interessado na _stream_. No pior dos casos, este _node_ seria o servidor.

#figure(
  caption: [Pacote enviado por parte de um cliente para parar uma _stream_],
  packet(
    "StopVideo",
    [id],
    [u64],
    [Identificador da _stream_],
  ),
)

De realçar, ainda, que tal como no caso do cliente, onde existe uma monitorização constante dos _point of presence_ existentes, aqui, de certa forma, também acontece. Essencialmente, caso um _point of presence_ detete que não recebe um _ping_ de um cliente interessado há pelo menos três intervalos de _ping_ #footnote[Valor atualmente definido para 3 segundos, 1 segundo por cada _ping_, mas facilmente configurável.], assume o cliente como morto, desativando os fluxos criados e, caso não existam outros clientes interessados nas ditas _streams_, envia um pedido de paragem para o seu parente, que se processa exatamente como mencionado anteriormente.

= Servidor

O servidor é o componente responsável por gerar os pacotes de streaming e enviá-los para a rede de entrega. Este componente é o único que tem acesso direto às _streams_ e é o responsável por propagar para a rede. O servidor é, também, o que envia pacotes de _flood_ periodicamente, como objetivo de manter a árvore de distribuição atualizada.

== Streaming

Para suporte de _streaming_ de vídeos em tempo real com alta qualidade, o grupo optou pelo uso do `ffmpeg` para a conversão de vídeos em qualquer formato (automaticamente detetados pelo `ffmpeg`) para o formato `h264` com `aac` para o áudio. Os _bytes_ gerados pelos _codecs_ são encapsulados em pacotes `MPEG-TS`.

Esta instância do `ffmpeg` é criada no início do programa, para todos os vídeos presentes na pasta de vídeos. Esta pasta de vídeos é passada como parâmetro ao programa. O programa também deteta mudanças nessa pasta e pode criar instâncias do `ffmpeg` para novos vídeos adicionados.

Para cada vídeo, também é gerado um identificador numérico único, para evitar que seja necessário enviar o nome da _stream_ em cada pacote de vídeo, gastando assim menos _bytes_ na rede. Este identificador é gerado a partir do nome do vídeo, sendo este um número de 64 bits. É de notar que é possível haver colisões de identificadores, mas devido ao número ser tão grande, a probabilidade de tal acontecer é extremamente baixa.

A instância do `ffmpeg` é então criada com o seguinte comando:
#figure(caption: [Comando para criação da instância do `ffmpeg`])[```bash
  ffmpeg -re -stream_loop -1 -i "<video_path>" -c:v "<codec_video>" -b:v 8M -c:a aac -f mpegts "<send_to_path>"
  ```]

A flag `-re` é usada para reproduzir o vídeo em tempo real, a flag `-stream_loop -1` é usada para repetir o vídeo indefinidamente, a flag `-i` é usada para indicar o caminho para o vídeo, `-c:v` é usado para indicar o codec de vídeo, `-b:v` é usado para indicar a _bitrate_ do vídeo, `-c:a` é usado para indicar o codec de áudio e `-f` é usado para indicar o formato de saída.

É de notar que o comando `ffmpeg` tem vários parâmetros:
- `<video_path>` é o caminho para o ficheiro do vídeo a ser transmitido;
- `<codec_video>` é o codec de vídeo a ser usado;
- `<send_to_path>` é o caminho para onde o vídeo é enviado.

O codec do vídeo é automaticamente detetado pelo programa, onde é sempre escolhido um codec `h264` que use sempre aceleração de hardware, caso esteja disponível. O programa executa o comando `ffmpeg -encoders` para obter a lista de codecs disponíveis e escolhe o primeiro codec presente nesta lista: "hevc_videotoolbox" (aceleração macOS), "h264_nvenc" (aceleração NVIDIA), "hevc_amf" (aceleração AMD), "h264" (aceleração CPU), "libx264" (aceleração CPU).

O `<send_to_path>` é o caminho para onde o vídeo é enviado. Este caminho será um _socket_ UDP que o servidor cria para receber os pacotes de streaming. Desta forma, o servidor recebe os pacotes de vídeo já particionados, para serem enviados via UDP, respeitando o MTU, não tendo de fazer trabalho para acumular _bytes_ do vídeo. A cada pacote recebido, o servidor envia o pacote encapsulado com a identificação da _stream_ para os _nodos_ que pediram a _stream_.

== Múltiplos servidores

A nossa arquitetura também possibilita a existência de múltiplos servidores. Cada servidor é responsável por um conjunto de vídeos, podendo ser o mesmo conjunto de vídeos que outro servidor ou não. Como cada _node_ tem a lista de vídeos que pode reproduzir, ao receber o pacote de _flood_ de um segundo servidor, apenas adiciona os vídeos desse servidor à sua lista de vídeos, e que pode reproduzir aquele vídeo a partir do _node_ que lhe enviou o pacote de _flood_.

== Adição de vídeos em runtime
Como funcionalidade extra, o grupo decidiu implementar a capacidade de adicionar _streams_ durante a execução do servidor, eliminando a necessidade de interromper qualquer transmissão que esteja a decorrer para tal.

Esta funcionalidade foi integrada de forma natural, uma vez que o servidor já propagava para a rede quais as _streams_ disponíveis aquando do _flooding_. _Flooding_ este que se processa de forma contínua no tempo, dando aso ao sistema de monitorização da rede que será abordado com mais detalhe num próximo capítulo.

#pagebreak()

= Monitorização da Rede
A grande vantagem de possuir um sistema de monitorização da rede contínuo no tempo é o facto da árvore de distribuição ótima se ir alterando de acordo com as condições da rede. Além disso, esta monitorização permite-nos, também, a deteção de falhas por parte de _nodes_ integrantes da rede de entrega. Esta deteção irá ser abordada com mais detalhe num próximo capítulo.

Ainda, o grupo chegou a uma solução tão simples quanto um pacote, apelidado de _flood_, que é enviado periodicamente para a rede com origem no servidor. Este pacote é enviado para cada um dos vizinhos de cada _node_ até chegar aos _point of presence_.

Este pacote contém informação relevante para a monitorização da rede, como o número de saltos que o pacote já deu, o _timestamp_ de criação do pacote no servidor para cálculo do delay, a lista de vídeos disponíveis para consumo e a lista de _nodes_ por onde o pacote já passou.

#figure(
  caption: [Pacote enviado pelo servidor para a rede para monitorização],
  packet(
    "FloodPacket",
    `created_at_server_time`,
    `SystemTime`,
    [_Timestamp_ de criação do pacote no servidor],
    `videos_available`,
    `Vec<(u64, String)>`,
    [Lista de pares identificador da _stream_, nome da _stream_ de vídeos que o nodo que enviou este pacote tem disponíveis],
    `visited_nodes`,
    `Vec<u64>`,
    [Lista de identificadores únicos dos _nodes_ por onde o pacote já passou],
    `my_parents`,
    `Vec<SocketAddr>`,
    [Lista de endereços dos _nodes_ pais do _node_ que enviou o pacote],
  ),
)

Este pacote é propagado para os vizinhos do _node_ que o recebeu, exceto para o _node_ que enviou o pacote. O pacote não é propagado caso o identificador do _node_ atual já esteja na lista de `visited_nodes`, para evitar ciclos.

== Fiabilidade com UDP
Uma vez que o grupo decidiu utilizar o protocolo de transporte UDP em praticamente todo o sistema, tornou-se desafiante a implementação de uma solução suficientemente genérica e performante.

Para tal, o grupo optou por implementar #link("https://github.com/chicoferreira/pessi-streamer/blob/main/common/src/reliable.rs")[uma abstração em cima do _socket_ UDP]. Esta abstração tem como noção a de pacotes _reliable_ e _unreliable_. Todas as entidades do sistema (exceto o _bootstrapper_) usam esta abstração para comunicação.

Os pacotes _reliable_ são pacotes que são enviados e que o emissor espera uma resposta, um _Ack_. Caso não receba esse _Ack_ num tempo determinado #footnote[O valor determinado para este tempo foi de 100ms, para haver um bom equilíbrio entre rapidez de resposta e evitar uso desnecessário da rede.], o pacote é reenviado, até um número de tentativas #footnote[Valor determinado de 5 tentativas, pelas mesmas razões da escolha anterior.], onde caso esse número seja atingido, o pacote é considerado perdido e o emissor do pacote é notificado.

Os pacotes _unreliable_ são pacotes que são enviados e esquecidos, como é no UDP.

Todos os pacotes, exceto os pacotes de vídeo, são enviados de forma _reliable_. Como o protocolo _mpeg-ts_ é resiliente a falhas, não é necessário retransmitir pacotes de vídeo, sendo assim então enviados de forma _unreliable_.

== Algoritmo de Escolha

Tanto os clientes, como os _nodes_ precisam de escolher quando o melhor próximo _node_ para contactar, quando é pedido um pacote de começar uma _stream_.

Para isso, é levado em conta o RTT entre os vários vizinhos (ou _point of presence_ no caso do cliente). Caso existam falhas na rede, como o pacote será retransmitido, o RTT será maior, e então o vizinho dificilmente será escolhido como melhor nó.

Desta forma, nos _nodes_ o melhor _node_ é escolhido a partir destes critérios:
+ Escolher os _nodes_ que a média dos últimos 10 RTTs esteja entre 30% da menor média de RTT;
+ Entre estes _nodes_ escolher o que tem menos _hops_ até ao servidor;
+ Se o número de _hops_ for igual, escolher o que tem menos vídeos pedidos atualmente;
+ Se o número de vídeos pedidos for igual, escolher o que tem menos _streams_ que pode enviar.

No caso dos clientes, é o melhor _point of presence_ é escolhido a partir dos mesmos críterios, excluindo o passo 3. já que os clientes não têm acesso ao número de _hops_ que um _PoP_ tem até ao servidor.

= Recuperação de Falhas

Tanto o cliente como os _nodes_ rodam uma tarefa de provisionamento de conexões.

Nos _nodes_, quando um vizinho lhe envia um pacote de _flood_, esse vizinho é considerado como um _node parent_. A partir deste ponto, é esperado que o _node parent_ lhe envie periodicamente pacotes de _flood_.

Caso um _node_ não receba um pacote de _flood_ de um _node parent_ durante um intervalo de tempo determinado #footnote[Valor determinado de 3 segundos, um equilíbrio entre deteção rápida para evitar interrupções nos vídeos do cliente e não marcar _nodes_ não suficientemente lentos como mortos.], o _node_ considera o _node parent_ como `Unresponsive`, e são feitos os procedimentos esclarecidos nas secções seguintes, de acordo com o tipo de mortes.

Caso um _node_ não faça _Ack_ do pacote de _flood_ enviado, o _node_ que enviou o pacote também considera o _node_ que não fez _Ack_ como `Unresponsive`.

Caso um _node_ não receba um pacote de vídeo que era suposto receber, em 500ms, tal como o cliente, o _node_ reenvia o pacote de pedido de vídeo. Isto serve para garantir que, caso o _node_ que lhe está a enviar o vídeo tenha reiniciado ou perdido o estado, o _node_ que pediu o vídeo consiga recuperar o estado.

== Mortes

Para exemplificar o procedimento em caso de mortes simples e mortes complexas, usaremos esta tipologia como exemplo:

#figure(image("assets/mortes.svg", width: 50%), caption: [Topologia de exemplo])

Nesta topologia, assume-se que o cliente $C_1$ está a pedir uma _stream_ ao servidor $S_1$ a partir dos _nodes_ $N_4 -> N_3 -> N_1 -> S_1$.

=== Mortes Simples
Assumindo agora que o $N_3$ morre, o $N_4$ vai detetar que o $N_3$ não lhe está a enviar pacotes de _flood_ e considera-o como `Unresponsive`. O $N_4$ vai então pedir ao $N_2$ para lhe enviar a _stream_ que estava a receber do $N_3$. O $N_2$ vai então pedir ao $N_1$ para lhe enviar a _stream_ que estava a receber do $N_3$. O $N_1$ como já estava a receber a _stream_ do $S$, vai então começar a enviar a _stream_ para o $N_2$, que vai enviar para o $N_4$, que vai enviar para o cliente. O $N_1$ também vai detetar que o $N_3$ não lhe está a enviar pacotes de _Ack_ para o pacote de _flood_, considera-o como `Unresponsive` e para de lhe enviar os pacotes de vídeo.

=== Mortes Catastróficas
Partindo do estado final mostrado na secção anterior, onde o $N_3$ está morto, podemos agora matar o $N_2$ para simular uma morte complexa.

Matando o $N_2$, o mesmo procedimento acontece no $N_1$ descrito anteriormente, mas agora o $N_4$ não tem vizinhos disponíveis para pedir o vídeo. Como o $N_4$ sabe que os parentes do $N_3$ é o $N_1$, o $N_4$ irá enviar o pacote de `NewNeighbour` para o $N_1$. O $N_1$ irá adicionar o $N_4$ à lista dos seus vizinhos. O $N_4$ adiciona as _streams_ perdidas a uma lista de _streams_ pendentes. Quando o $N_1$ receber um pacote de _flood_ do $S_1$, agora como o $N_4$ é seu vizinho, irá enviar-lhe o pacote de _flood_. O $N_4$ recebendo esse pacote de _flood_ vai então pedir a _stream_ ao $N_1$ e o $N_1$ vai enviar a _stream_ para o $N_4$.

Agora, se o $N_1$ morrer também, o $N_4$ tem a lista de parentes do $N_1$ a partir do pacote de _flood_ e então poderá repetir o mesmo algoritmo.

Caso um _node_ morto volte à vida, os _nodes_ vizinhos detetam que ele voltou à vida e declaram-no como `Responsive` novamente. Nenhum vídeo é reencaminhado e nenhuma conexão é refeita. No exemplo anterior, se o $N_2$ voltar à vida, o $N_1$ continuará a mandar pacotes de _flood_ para o $N_4$ visto ainda pertencer à lista de vizinhos dele. Só no caso de algum _node_ morrer ou algum cliente pedir um novo vídeo, é que o _nodo_ que voltou à vida pode ser usado.

== Adições de vizinhos em _runtime_

Caso um _node_ não inicie corretamente, os vizinhos desse _node_ declararão o _node_ como `Unresponsive`. O _node_ assim que iniciar, irá reencaminhar o pacote de _flood_ para os vizinhos, que irão declarar o _node_ como `Responsive` novamente, podendo assim pertencer à rede normalmente.

= Segurança na Rede

Um fator que o grupo não explorou o suficiente foi a segurança na rede. A segurança é um fator muito importante num cenário real, onde pacotes podem ser facilmente intercetados e alterados. A arquitetura é altamente baseada nos IPs que vão nos pacotes UDPs e isto é um problema de segurança, já que estes podem ser facilmente alterados, a partir de um agente malicioso.

Num cenário real, seria necessário implementar um sistema de autenticação e encriptação dos pacotes, para garantir que os pacotes são enviados por quem dizem ser e que não são alterados durante o envio. Isto poderia ser feito a partir da migração do uso de MPEG-TS para o HLS, em que a _stream_ seria enviada a partir de HTTPS, garantindo a autenticação e encriptação dos pacotes. Entre os _nodes_, o mesmo HTTPS poderia ser usado, ou então um sistema de autenticação e encriptação próprio.

Como esta UC não engloba estes temas, o grupo não entrou muito a fundo neste tópico. No entanto, existe uma #link("https://github.com/chicoferreira/pessi-streamer/tree/rl/encryption")[_branch_] (não contendo os _commits_ mais recentes) onde os pacotes são encriptados a partir de uma chave simétrica partilhada entre os _nodes_. Não incluímos na solução final porque não é resiliente a _replay attacks_, não tem autenticação nem integridade, não tem _forward secrecy_, e só prejudicaria a performance do sistema com pouco benefício ganho.

#pagebreak()

= Protocolo e serialização de pacotes

Para a comunicação entre os diversos componentes do sistema, foi necessário optar por um protocolo de comunicação extremamente eficiente e de fácil implementação. Para tal, o grupo decidiu utilizar a _crate_ #footnote[O termo _crate_ é usado em Rust para referir uma biblioteca.] #link("https://github.com/bincode-org/bincode")[bincode], que é uma biblioteca de serialização binária para Rust. Esta _crate_, juntamente com o   #link("https://serde.rs/")[_serde_]#footnote[O #link("https://serde.rs/")[_serde_] é uma _crate_ que permite a serialização e desserialização de dados para formatos genéricos em Rust.], permitiu com que o código de serialização e desserialização de pacotes fosse gerado em _compile-time_ a partir das definições dos _enums_ e _structs_.

A especificação da serialização de _structs_ e _enums_ para _bytes_ com vários exemplos está presente em https://github.com/bincode-org/bincode/blob/trunk/docs/spec.md.

Todos os pacotes estão definidos no ficheiro #link("https://github.com/chicoferreira/pessi-streamer/blob/main/common/src/packet.rs")[pessi-streamer/common/src/packet.rs]. Estes pacotes, como são anotados pelo _macro_ `#[Serialize, Deserialize]`, será gerado o código de serialização e desserialização automaticamente. 

Estes pacotes, como são enviados a partir da abstração do _socket_ UDP, serão ainda encapsulados no pacote definido em: #link("https://github.com/chicoferreira/pessi-streamer/blob/main/common/src/reliable.rs#L41")[pessi-streamer/common/src/reliable.rs\#L41].

Isto permite-nos ter um protocolo binário extremamente eficiente com muito pouco esforço.

= Conclusão

Para concluir, o grupo desenvolveu um sistema de distribuição de _streams_ multimédia em tempo real, com capacidade de adição de _streams_ em _runtime_, monitorização da rede, recuperação de falhas e uma interface gráfica para o utilizador final. As _streams_ são reproduzidas em altissima qualidade. O sistema é altamente escalável e performante, com a possibilidade de adicionar múltiplos servidores e _point of presence_. 

A escolha da linguagem Rust foi uma escolha muito acertada. Seja pela facilidade da serialização de dados num formato binário eficiente, seja pelo sistema de tipos concreto que faz com que se evite erros comuns durante o desenvolvimento ou seja pela facilidade de programar certos tipos de _patterns_ de programação concorrente, o grupo irá continuar a usá-la em projetos futuros.

Com isto, o grupo achou que este trabalho foi um sucesso, tendo cumprido todos os objetivos propostos e tendo ultrapassado as expectativas iniciais.