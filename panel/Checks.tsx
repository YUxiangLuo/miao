import { useEffect, useState } from "react";

type NetCheck = {
    id: number;
    status: number;
    time: string;
}

export default function Checks() {

    const [net_checks, setNetChecks] = useState<NetCheck[]>([]);

    useEffect(() => {
        const si = setInterval(async () => {
            const res = await fetch("/api/checks");
            const res_json = (await res.json()) as NetCheck[];
            setNetChecks(res_json);
        }, 1000);
        return () => {
            clearInterval(si);
        }
    }, []);

    const net_check_elements = net_checks.map(x => <div key={x.id}>{x.id}, {x.status === 1 ? "成功" : "失败"}, {new Date(x.time).toLocaleString()}</div>)

    return (
        <div>
            <h1>最近10次连通性测试</h1>
            {net_check_elements}
        </div>
    )
}