# CANELA AI — v0.7: agentes especializados (rama de desarrollo)

Estado: **arquitectura + primera implementación funcional en la rama de desarrollo**. No es una versión estable: falta probarla en Windows 11 real. La versión validada sigue siendo la **v0.5**.

## 1. Idea

Un **agente** no es otro chat ni otro modelo cargado en memoria. Es un **perfil**: instrucciones + lista de herramientas permitidas. Para cada mensaje, un **orquestador** elige el perfil y la respuesta sigue en la **misma conversación** y con el **mismo modelo** que eligió el enrutador (v0.6). Así no se cargan varios modelos grandes a la vez.

```
Mensaje ─► Enrutador (modelo, privacidad, presupuesto)
              │
              ▼
         Orquestador ─► agente: programadora · investigadora · productividad · archivos · sistema · Canela general
              │
              ▼
         Bucle de agente ─► herramientas permitidas al agente ─► confirmación (si es delicada) ─► ejecución
              │                                                                                    │
              │                                                       Verificación objetiva ◄──────┘
              ▼
         Informe «Qué hice» + avisos (afirmaciones sin acción, fuentes no consultadas, comprobaciones fallidas)
```

## 2. Agentes y permisos

| Agente | Herramientas (las demás no las ve ni puede usarlas) |
|---|---|
| Programadora | leer/listar/buscar archivos, leer documentos, escribir archivo (con diferencias y aprobación), crear carpeta, analizar proyecto, abrir con VS Code, índice de documentos, recuerdos, planes |
| Investigadora | **buscar_wikipedia**, **leer_pagina** (nuevas), abrir web, buscar/leer documentos propios, recuerdos |
| Productividad | tareas (crear, listar, actualizar, eliminar), recuerdos, conversaciones, planes y rutinas, abrir aplicaciones, multimedia |
| Archivos | listar, buscar, recientes, leer, ver imagen, crear carpeta, organizar (sólo mueve), escribir, abrir, índice de documentos. **No tiene herramienta de borrar.** |
| Sistema | info_sistema, **diagnostico_memoria** (nueva, sólo lectura), multimedia, abrir aplicaciones, ver pantalla (con permiso) |
| Canela general | todas (como en v0.6), para charla o cuando el orquestador no está seguro |

**Garantías comprobadas en el backend** (no dependen de que el modelo obedezca):
- El modelo sólo recibe las definiciones de las herramientas de su agente.
- Si aun así pide otra, se rechaza («no permitida») y queda en el informe.
- Un plan (`ejecutar_plan`) tampoco puede incluir herramientas de otro agente: se rechaza el plan entero antes de ejecutar ningún paso.
- Todas las confirmaciones existentes siguen igual (borrar, escribir, mover, capturas, enviar datos fuera).

## 3. Contexto compartido de forma controlada

- Todos los agentes ven la conversación (lo que escribiste y las respuestas finales).
- Los **datos internos de las herramientas** (contenido de archivos leídos, resultados intermedios) **sólo los vuelve a ver el mismo agente** que los obtuvo. Si cambia el agente, sólo pasa la respuesta final.
- Mensajes cortos de seguimiento («sí, hazlo», «¿y el otro?») mantienen el agente anterior.
- Con un modelo externo, todo lo leído por herramientas sigue pasando por el filtro de privacidad de la v0.6.1.

## 4. Nada de «fingir que trabaja»

- **Registro real**: cada acción que se ejecuta (o se rechaza, o deniegas) queda registrada por el backend, no por el modelo.
- **Verificación objetiva** después de actuar:
  - crear tarea → la tarea existe en la base de datos con ese título;
  - eliminar tarea → ya no existe;
  - escribir archivo → el archivo existe y su contenido es exactamente el aprobado;
  - crear carpeta → la carpeta existe;
  - recordar → el recuerdo está guardado;
  - guardar rutina → la rutina existe.
  Las demás acciones (abrir una app, leer) se marcan como «no comprobable» en vez de inventar una comprobación.
- **Informe «Qué hice»** bajo la respuesta: acciones hechas, cuántas comprobadas, cuántas fallaron la comprobación y cuántas no se hicieron.
- **Avisos automáticos**:
  - si la respuesta dice «ya lo guardé / creé / borré…» pero no se ejecutó ninguna acción;
  - si la investigadora cita una URL que no consultó en esa respuesta;
  - si una comprobación falla.

## 5. Herramientas nuevas (investigación y sistema)

- `buscar_wikipedia` (API pública de Wikipedia) y `leer_pagina`: sólo `http/https` públicos; se rechazan `localhost`, redes privadas (10.x, 192.168.x, 172.16–31.x, 169.254.x, `::1`, `fc00::/7`, `.local`, `.internal`), direcciones con usuario/contraseña y, tras redirecciones, también la dirección final. Máx. 2 MB y 15 s; se quita el HTML (scripts, estilos) y se recorta a 20 000 caracteres. El contenido se trata como **externo** (no puede dar órdenes) y la consulta se revisa con el filtro de privacidad. Piden confirmación salvo que hayas activado abrir webs sin preguntar.
- `diagnostico_memoria`: lee la última muestra del diagnóstico de la v0.5 (RAM, commit, procesos por nombre, sin rutas) y los últimos eventos. Sólo lectura.

## 6. Evaluación (automática, Linux, modelos simulados)

| Prueba | Resultado |
|---|---|
| Orquestador, conjunto usado al desarrollarlo (30 mensajes) | 29/30 (97 %, optimista) |
| **Orquestador, conjunto nuevo escrito después y medido una vez sin reajustar (30)** | **25/30 (83 %)**. Los 5 fallos fueron a «Canela general» (que tiene todas las herramientas), ninguno a un agente equivocado |
| Cada agente sólo ve sus herramientas (petición real al modelo simulado) | ✔ |
| Herramienta ajena pedida por el modelo → rechazada, sin efecto | ✔ |
| Plan con un paso ajeno → rechazado entero, ningún paso ejecutado | ✔ |
| Crear tarea → comprobada en la base de datos y en el informe | ✔ |
| «Ya creé la tarea» sin acción → aviso | ✔ |
| Fuente citada sin consultar → aviso | ✔ |
| Datos internos de otro agente no se comparten; los del mismo, sí | ✔ |
| Seguridad de `leer_pagina` (12 direcciones peligrosas rechazadas) | ✔ |
| **En la app (Linux, Ollama simulado)**: «crea una tarea…» → «asistente de productividad», 14 herramientas enviadas, «✓ Crear tarea · comprobado», «Qué hice: 1 acción hecha · 1 comprobada» | ✔ |
| **En la app**: respuesta que dice «ya guardé el informe» sin acción → «agente de archivos» + aviso visible | ✔ |

## 7. Pendiente

- **Probar en Windows 11 real** con Qwen3 y con Claude/ChatGPT: que el modelo real use bien las herramientas de cada agente y que los avisos no molesten.
- Que la investigadora funcione con internet real (en las pruebas, Wikipedia está simulada).
- Mejorar el orquestador con los fallos del conjunto nuevo (en un conjunto distinto para volver a medir sin trampa).
- v0.8 (siguiente del plan): verificación más amplia (¿compila?, ¿pasan las pruebas?) y reintentos acotados.
