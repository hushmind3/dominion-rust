import {post} from '../api'

// Only the user's explicit repair action creates an RSI issue.
export async function openImprovementIssue(body:{signalId:string}|{sessionId:string;description:string}){
 const issue=await post<{id:string}>('/api/rsi/issues',body)
 window.location.hash='#/rsi?issue='+encodeURIComponent(issue.id)
}
